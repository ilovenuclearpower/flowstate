use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::{bail, Result};
use flowstate_core::claude_run::{ClaudeAction, ClaudeRun};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_prompts::{ChildTaskInfo, ParentContext, PromptContext};
use flowstate_service::{HttpService, TaskService};
use flowstate_verify::Runner as VerifyRunner;
use tracing::{error, info};

use crate::backend::AgentBackend;
use crate::plan_parser;
use crate::repo_provider::{self, ProviderError};
use crate::workspace;

/// Execute the full build pipeline for an approved task.
#[allow(clippy::too_many_arguments)]
pub async fn execute(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    timeout: Duration,
    kill_grace: Duration,
    backend: &dyn AgentBackend,
) -> Result<()> {
    // 1. Validate prerequisites
    //    Subtasks inherit approvals from their parent task.
    let is_subtask = task.is_subtask();
    if is_subtask {
        let parent_id = task.parent_id.as_deref().unwrap();
        let parent = service
            .get_task(parent_id)
            .await
            .map_err(|e| anyhow::anyhow!("failed to fetch parent task: {e}"))?;
        if parent.spec_status != flowstate_core::task::ApprovalStatus::Approved {
            bail!("parent spec must be approved before building subtask");
        }
        if parent.plan_status != flowstate_core::task::ApprovalStatus::Approved {
            bail!("parent plan must be approved before building subtask");
        }
    } else {
        if task.spec_status != flowstate_core::task::ApprovalStatus::Approved {
            bail!("spec must be approved before building");
        }
        if task.plan_status != flowstate_core::task::ApprovalStatus::Approved {
            bail!("plan must be approved before building");
        }
    }

    // 2. Fetch repo token (PAT) — used for both provider auth and git clone
    let token = service.get_repo_token(&project.id).await.ok();

    // 3. Resolve repo provider and check auth
    progress(service, &run.id, "Checking repo auth...").await;
    let provider = repo_provider::provider_for_url(
        &project.repo_url,
        token.clone(),
        project.provider_type,
        project.skip_tls_verify,
    )
    .map_err(|e| anyhow::anyhow!("unsupported repo provider: {e}"))?;

    provider
        .preflight()
        .await
        .map_err(|e| anyhow::anyhow!("provider preflight: {e}"))?;

    provider.check_auth(&project.repo_url).await.map_err(|e| {
        anyhow::anyhow!("repo auth check failed: {e}")
    })?;

    // 4. Clone repo to fresh workspace
    progress(service, &run.id, "Cloning repository...").await;
    workspace::ensure_repo(ws_dir, &project.repo_url, token.as_deref(), project.skip_tls_verify)
        .await?;

    // 5. Detect default branch (fresh clone is already on it)
    let default_branch = workspace::detect_default_branch(ws_dir).await?;

    // 6. Create feature branch
    progress(service, &run.id, "Creating feature branch...").await;
    let branch_name = format!("flowstate/{}", slugify(&task.title));
    workspace::create_branch(ws_dir, &branch_name).await?;

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

    // For subtasks, fetch parent context (spec + plan) to provide broader scope
    let parent_context = if is_subtask {
        let parent_id = task.parent_id.as_deref().unwrap();
        let parent = service.get_task(parent_id).await.ok();
        let parent_spec = service.read_task_spec(parent_id).await.ok();
        let parent_plan = service.read_task_plan(parent_id).await.ok();
        parent.map(|p| ParentContext {
            title: p.title,
            description: p.description,
            spec_content: parent_spec,
            plan_content: parent_plan,
        })
    } else {
        None
    };

    let ctx = PromptContext {
        project_name: project.name.clone(),
        repo_url: project.repo_url.clone(),
        task_title: task.title.clone(),
        task_description: task.description.clone(),
        spec_content,
        plan_content: plan_content.clone(),
        research_content: None,
        verification_content: None,
        distill_feedback: None,
        child_tasks,
        parent_context,
    };

    let prompt = flowstate_prompts::assemble_prompt(&ctx, ClaudeAction::Build);

    // 9. Save prompt
    save_run_prompt(&run.id, &prompt)?;

    // 10. Run agent in workspace
    progress(service, &run.id, &format!("Running {}...", backend.name())).await;
    info!("running {} for build in {}", backend.name(), ws_dir.display());
    let output = backend.run(&prompt, ws_dir, timeout, kill_grace).await?;

    if !output.success {
        let msg = if output.stderr.is_empty() {
            format!("agent exited with code {}", output.exit_code)
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
        let result = verifier.execute(&validation_steps, ws_dir).await;

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
    workspace::add_and_commit(ws_dir, &commit_msg).await?;

    // 15. Push branch
    progress(service, &run.id, "Pushing branch...").await;
    provider.push_branch(ws_dir, &branch_name).await.map_err(|e| {
        anyhow::anyhow!("push failed: {e}")
    })?;

    // 16. Open PR
    progress(service, &run.id, "Opening pull request...").await;
    let pr_body = format!(
        "## Task\n\n{}\n\n## Description\n\n{}\n\n---\nGenerated by flowstate runner",
        task.title, task.description
    );
    let pr = provider
        .open_pull_request(ws_dir, &branch_name, &task.title, &pr_body, &default_branch)
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

    // 17b. Link PR to task
    progress(service, &run.id, "Linking PR to task...").await;
    let create_pr = flowstate_core::task_pr::CreateTaskPr {
        task_id: task.id.clone(),
        claude_run_id: Some(run.id.clone()),
        pr_url: pr.url.clone(),
        pr_number: pr.number as i64,
        branch_name: pr.branch.clone(),
    };
    if let Err(e) = service.create_task_pr(&create_pr).await {
        tracing::warn!("failed to link PR to task: {e}");
    }

    // 18. Update task status to Verify
    let update = flowstate_core::task::UpdateTask {
        status: Some(flowstate_core::task::Status::Verify),
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn slugify_simple() {
        assert_eq!(slugify("My Task"), "my-task");
    }

    #[test]
    fn slugify_special_chars() {
        assert_eq!(slugify("Hello, World!"), "hello-world");
    }

    #[test]
    fn slugify_consecutive_dashes() {
        assert_eq!(slugify("a--b"), "a-b");
    }

    #[test]
    fn slugify_truncates_at_50() {
        let long_title = "a".repeat(100);
        let result = slugify(&long_title);
        assert_eq!(result.len(), 50);
        assert!(result.chars().all(|c| c == 'a'));
    }

    #[test]
    fn slugify_empty() {
        assert_eq!(slugify(""), "");
    }
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
