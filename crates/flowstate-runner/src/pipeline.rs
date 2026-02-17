use std::path::PathBuf;

use anyhow::{bail, Result};
use flowstate_core::claude_run::{ClaudeAction, ClaudeRun};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_prompts::{ChildTaskInfo, PromptContext};
use flowstate_service::{HttpService, TaskService};
use flowstate_verify::Runner as VerifyRunner;
use tracing::{error, info};

use crate::executor::run_claude;
use crate::plan_parser;
use crate::repo_provider::{self, ProviderError};
use crate::workspace;

/// Execute the full build pipeline for an approved task.
pub async fn execute(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    workspace_root: &Option<PathBuf>,
) -> Result<()> {
    // 1. Validate prerequisites
    if task.spec_status != flowstate_core::task::ApprovalStatus::Approved {
        bail!("spec must be approved before building");
    }
    if task.plan_status != flowstate_core::task::ApprovalStatus::Approved {
        bail!("plan must be approved before building");
    }

    // 2-3. Resolve repo provider and check auth
    progress(service, &run.id, "Checking repo auth...").await;
    let provider = repo_provider::provider_for_url(&project.repo_url).map_err(|e| {
        anyhow::anyhow!("unsupported repo provider: {e}")
    })?;

    // 3. Verify repo auth
    provider.check_auth(&project.repo_url).await.map_err(|e| {
        anyhow::anyhow!("repo auth check failed: {e}")
    })?;

    // 4. Clone/pull repo to workspace
    progress(service, &run.id, "Cloning/pulling repository...").await;
    let ws_dir = resolve_workspace_dir(workspace_root, &project.id);
    workspace::ensure_repo(&ws_dir, &project.repo_url).await?;

    // 5. Checkout default branch and detect it
    let default_branch = workspace::checkout_default_branch(&ws_dir).await?;

    // Pull latest on default branch
    workspace::ensure_repo(&ws_dir, &project.repo_url).await?;

    // 6. Create feature branch
    progress(service, &run.id, "Creating feature branch...").await;
    let branch_name = format!("flowstate/{}", slugify(&task.title));
    workspace::create_branch(&ws_dir, &branch_name).await?;

    // 7. Read spec + plan from server
    let spec_content = service
        .read_task_spec(&task.id)
        .await
        .ok();
    let plan_content = service
        .read_task_plan(&task.id)
        .await
        .ok();

    // 8. Assemble build prompt
    progress(service, &run.id, "Assembling build prompt...").await;
    let child_tasks = service
        .list_child_tasks(&task.id)
        .await
        .unwrap_or_default()
        .into_iter()
        .map(|c| ChildTaskInfo {
            title: c.title,
            description: c.description,
            status: c.status.as_str().to_string(),
        })
        .collect();

    let ctx = PromptContext {
        project_name: project.name.clone(),
        repo_url: project.repo_url.clone(),
        task_title: task.title.clone(),
        task_description: task.description.clone(),
        spec_content,
        plan_content: plan_content.clone(),
        child_tasks,
    };

    let prompt = flowstate_prompts::assemble_prompt(&ctx, ClaudeAction::Build);

    // 9. Save prompt
    save_run_prompt(&run.id, &prompt)?;

    // 10. Run claude in workspace
    progress(service, &run.id, "Running Claude CLI...").await;
    info!("running claude for build in {}", ws_dir.display());
    let output = run_claude(&prompt, &ws_dir).await?;

    if !output.success {
        let msg = if output.stderr.is_empty() {
            format!("claude exited with code {}", output.exit_code)
        } else {
            output.stderr.clone()
        };
        service
            .update_claude_run_status(&run.id, "failed", Some(&msg), Some(output.exit_code))
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;
        return Ok(());
    }

    // 11-12. Parse plan for validation commands and run them
    let validation_steps = plan_content
        .as_deref()
        .map(plan_parser::extract_validation_commands)
        .unwrap_or_default();

    if !validation_steps.is_empty() {
        progress(service, &run.id, "Running validation tests...").await;
        info!("running {} validation steps", validation_steps.len());
        let verifier = VerifyRunner::new();
        let result = verifier.execute(&validation_steps, &ws_dir).await;

        match result.status {
            flowstate_verify::runner::RunStatus::Passed => {
                info!("all validation steps passed");
            }
            _ => {
                // 13. Tests failed — report failure, do NOT push
                let mut error_msg = String::from("Validation failed:\n");
                for step in &result.steps {
                    if step.exit_code != Some(0) {
                        error_msg.push_str(&format!(
                            "\n--- {} (exit {}) ---\n{}\n{}\n",
                            step.step_name,
                            step.exit_code.map_or("timeout".to_string(), |c| c.to_string()),
                            step.stdout,
                            step.stderr,
                        ));
                    }
                }
                error!("validation failed, not pushing");
                service
                    .update_claude_run_status(&run.id, "failed", Some(&error_msg), Some(1))
                    .await
                    .map_err(|e| anyhow::anyhow!("{e}"))?;
                return Ok(());
            }
        }
    }

    // 14. Tests pass -> commit
    progress(service, &run.id, "Committing changes...").await;
    let commit_msg = format!("feat: {} [flowstate]", task.title);
    workspace::add_and_commit(&ws_dir, &commit_msg).await?;

    // 15. Push branch
    progress(service, &run.id, "Pushing branch...").await;
    provider.push_branch(&ws_dir, &branch_name).await.map_err(|e| {
        anyhow::anyhow!("push failed: {e}")
    })?;

    // 16. Open PR
    progress(service, &run.id, "Opening pull request...").await;
    let pr_body = format!(
        "## Task\n\n{}\n\n## Description\n\n{}\n\n---\nGenerated by flowstate runner",
        task.title, task.description
    );
    let pr = provider
        .open_pull_request(&ws_dir, &branch_name, &task.title, &pr_body, &default_branch)
        .await
        .map_err(|e: ProviderError| anyhow::anyhow!("PR creation failed: {e}"))?;

    // 17. Update claude_run with PR info
    service
        .update_claude_run_pr(
            &run.id,
            Some(&pr.url),
            Some(pr.number as i64),
            Some(&pr.branch),
        )
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    // 18. Update task status to InReview
    let update = flowstate_core::task::UpdateTask {
        status: Some(flowstate_core::task::Status::InReview),
        ..Default::default()
    };
    service
        .update_task(&task.id, &update)
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;

    info!(
        "build pipeline complete: PR #{} at {}",
        pr.number, pr.url
    );

    Ok(())
}

async fn progress(service: &HttpService, run_id: &str, message: &str) {
    info!("{message}");
    let _ = service.update_claude_run_progress(run_id, message).await;
}

fn resolve_workspace_dir(workspace_root: &Option<PathBuf>, project_id: &str) -> PathBuf {
    match workspace_root {
        Some(root) => root.join(project_id),
        None => {
            if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
                PathBuf::from(xdg)
                    .join("flowstate")
                    .join("workspaces")
                    .join(project_id)
            } else if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(home)
                    .join(".local/share/flowstate/workspaces")
                    .join(project_id)
            } else {
                PathBuf::from(".")
                    .join("flowstate/workspaces")
                    .join(project_id)
            }
        }
    }
}

fn slugify(title: &str) -> String {
    title
        .to_lowercase()
        .chars()
        .map(|c| if c.is_alphanumeric() { c } else { '-' })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
        .chars()
        .take(50)
        .collect()
}

fn save_run_prompt(run_id: &str, prompt: &str) -> Result<()> {
    let data_dir = if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("flowstate")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share/flowstate")
    } else {
        PathBuf::from(".").join("flowstate")
    };
    let run_dir = data_dir.join("claude_runs").join(run_id);
    std::fs::create_dir_all(&run_dir)?;
    std::fs::write(run_dir.join("prompt.md"), prompt)?;
    Ok(())
}
