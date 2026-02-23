use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use flowstate_core::claude_run::{ClaudeAction, ClaudeRun};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_prompts::{ChildTaskInfo, PromptContext};
use flowstate_service::{HttpService, TaskService};
use tracing::{info, warn};

use crate::backend::AgentBackend;
use crate::config::RunnerConfig;
use crate::pipeline;
use crate::workspace;

/// Dispatch a claimed run to the appropriate handler.
/// Each run gets a fresh workspace directory keyed by run ID,
/// which is cleaned up after the run completes (success or failure).
pub async fn dispatch(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    config: &RunnerConfig,
    backend: &dyn AgentBackend,
) -> Result<()> {
    let ws_dir = resolve_workspace_dir(&config.workspace_root, &run.id);
    info!("workspace for run {}: {}", run.id, ws_dir.display());

    let timeout = config.timeout_for_action(run.action);
    let kill_grace = Duration::from_secs(config.kill_grace_period);

    let result = match run.action {
        ClaudeAction::Research | ClaudeAction::ResearchDistill => {
            execute_research(service, run, task, project, &ws_dir, timeout, kill_grace, backend)
                .await
        }
        ClaudeAction::Design | ClaudeAction::DesignDistill => {
            execute_design(service, run, task, project, &ws_dir, timeout, kill_grace, backend)
                .await
        }
        ClaudeAction::Plan | ClaudeAction::PlanDistill => {
            execute_plan(service, run, task, project, &ws_dir, timeout, kill_grace, backend).await
        }
        ClaudeAction::Build => {
            pipeline::execute(service, run, task, project, &ws_dir, timeout, kill_grace, backend)
                .await
        }
        ClaudeAction::Verify | ClaudeAction::VerifyDistill => {
            execute_verify(service, run, task, project, &ws_dir, timeout, kill_grace, backend)
                .await
        }
    };

    // Always clean up workspace after the run
    cleanup_workspace(&ws_dir);

    result
}

/// Resolve a per-run workspace directory.
pub fn resolve_workspace_dir(workspace_root: &Option<PathBuf>, run_id: &str) -> PathBuf {
    match workspace_root {
        Some(root) => root.join(run_id),
        None => {
            if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
                PathBuf::from(xdg)
                    .join("flowstate")
                    .join("workspaces")
                    .join(run_id)
            } else if let Some(home) = std::env::var_os("HOME") {
                PathBuf::from(home)
                    .join(".local/share/flowstate/workspaces")
                    .join(run_id)
            } else {
                PathBuf::from(".")
                    .join("flowstate/workspaces")
                    .join(run_id)
            }
        }
    }
}

pub fn cleanup_workspace(dir: &Path) {
    if dir.exists() {
        info!("cleaning up workspace: {}", dir.display());
        if let Err(e) = std::fs::remove_dir_all(dir) {
            warn!("failed to remove workspace {}: {e}", dir.display());
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn execute_research(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    timeout: Duration,
    kill_grace: Duration,
    backend: &dyn AgentBackend,
) -> Result<()> {
    // Clone repo so the agent can explore the codebase
    progress(service, &run.id, "Cloning repository...").await;
    let token = service.get_repo_token(&project.id).await.ok();
    workspace::ensure_repo(ws_dir, &project.repo_url, token.as_deref(), project.skip_tls_verify).await?;

    progress(service, &run.id, "Assembling prompt...").await;
    let ctx = build_prompt_context(service, task, project, run.action).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, run.action);

    save_prompt(&run.id, &prompt)?;

    progress(service, &run.id, &format!("Running {}...", backend.name())).await;
    let output = backend.run(&prompt, ws_dir, timeout, kill_grace).await?;

    if output.success {
        progress(service, &run.id, "Reading output...").await;
        let research_file = ws_dir.join("RESEARCH.md");
        let content = std::fs::read_to_string(&research_file)
            .unwrap_or_else(|_| output.stdout.clone());

        progress(service, &run.id, "Writing research to server...").await;
        service
            .write_task_research(&task.id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let update = flowstate_core::task::UpdateTask {
            research_status: Some(flowstate_core::task::ApprovalStatus::Pending),
            ..Default::default()
        };
        service
            .update_task(&task.id, &update)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        report_success(service, &run.id, output.exit_code).await?;
        info!("research complete for task {}", task.id);
    } else {
        report_failure(service, &run.id, &output.stderr, output.exit_code).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_design(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    timeout: Duration,
    kill_grace: Duration,
    backend: &dyn AgentBackend,
) -> Result<()> {
    // Clone repo so the agent can explore the codebase
    progress(service, &run.id, "Cloning repository...").await;
    let token = service.get_repo_token(&project.id).await.ok();
    workspace::ensure_repo(ws_dir, &project.repo_url, token.as_deref(), project.skip_tls_verify).await?;

    progress(service, &run.id, "Assembling prompt...").await;
    let ctx = build_prompt_context(service, task, project, run.action).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, run.action);

    save_prompt(&run.id, &prompt)?;

    progress(service, &run.id, &format!("Running {}...", backend.name())).await;
    let output = backend.run(&prompt, ws_dir, timeout, kill_grace).await?;

    if output.success {
        progress(service, &run.id, "Reading output...").await;
        let spec_file = ws_dir.join("SPECIFICATION.md");
        let content = std::fs::read_to_string(&spec_file)
            .unwrap_or_else(|_| output.stdout.clone());

        progress(service, &run.id, "Writing spec to server...").await;
        service
            .write_task_spec(&task.id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let update = flowstate_core::task::UpdateTask {
            spec_status: Some(flowstate_core::task::ApprovalStatus::Pending),
            ..Default::default()
        };
        service
            .update_task(&task.id, &update)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        report_success(service, &run.id, output.exit_code).await?;
        info!("design complete for task {}", task.id);
    } else {
        report_failure(service, &run.id, &output.stderr, output.exit_code).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_plan(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    timeout: Duration,
    kill_grace: Duration,
    backend: &dyn AgentBackend,
) -> Result<()> {
    // Clone repo so the agent can explore the codebase
    progress(service, &run.id, "Cloning repository...").await;
    let token = service.get_repo_token(&project.id).await.ok();
    workspace::ensure_repo(ws_dir, &project.repo_url, token.as_deref(), project.skip_tls_verify).await?;

    progress(service, &run.id, "Assembling prompt...").await;
    let ctx = build_prompt_context(service, task, project, run.action).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, run.action);

    save_prompt(&run.id, &prompt)?;

    progress(service, &run.id, &format!("Running {}...", backend.name())).await;
    let output = backend.run(&prompt, ws_dir, timeout, kill_grace).await?;

    if output.success {
        progress(service, &run.id, "Reading output...").await;
        let plan_file = ws_dir.join("PLAN.md");
        let content = std::fs::read_to_string(&plan_file)
            .unwrap_or_else(|_| output.stdout.clone());

        progress(service, &run.id, "Writing plan to server...").await;
        service
            .write_task_plan(&task.id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let update = flowstate_core::task::UpdateTask {
            plan_status: Some(flowstate_core::task::ApprovalStatus::Pending),
            ..Default::default()
        };
        service
            .update_task(&task.id, &update)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        report_success(service, &run.id, output.exit_code).await?;
        info!("plan complete for task {}", task.id);
    } else {
        report_failure(service, &run.id, &output.stderr, output.exit_code).await?;
    }

    Ok(())
}

#[allow(clippy::too_many_arguments)]
async fn execute_verify(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    timeout: Duration,
    kill_grace: Duration,
    backend: &dyn AgentBackend,
) -> Result<()> {
    // Clone repo so the agent can explore the codebase
    progress(service, &run.id, "Cloning repository...").await;
    let token = service.get_repo_token(&project.id).await.ok();
    workspace::ensure_repo(ws_dir, &project.repo_url, token.as_deref(), project.skip_tls_verify).await?;

    // Checkout the feature branch from the most recent completed build run
    let runs = service.list_claude_runs(&task.id).await.unwrap_or_default();
    let build_branch = runs
        .iter()
        .rfind(|r| {
            r.action == ClaudeAction::Build
                && r.status == flowstate_core::claude_run::ClaudeRunStatus::Completed
        })
        .and_then(|r| r.branch_name.clone());
    if let Some(ref branch) = build_branch {
        progress(service, &run.id, "Checking out feature branch...").await;
        let status = tokio::process::Command::new("git")
            .args(["checkout", branch])
            .current_dir(ws_dir)
            .status()
            .await?;
        if !status.success() {
            warn!("failed to checkout branch {branch}, continuing on default branch");
        }
    }

    progress(service, &run.id, "Assembling prompt...").await;
    let ctx = build_prompt_context(service, task, project, run.action).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, run.action);

    save_prompt(&run.id, &prompt)?;

    progress(service, &run.id, &format!("Running {}...", backend.name())).await;
    let output = backend.run(&prompt, ws_dir, timeout, kill_grace).await?;

    if output.success {
        progress(service, &run.id, "Reading output...").await;
        let verification_file = ws_dir.join("VERIFICATION.md");
        let content = std::fs::read_to_string(&verification_file)
            .unwrap_or_else(|_| output.stdout.clone());

        progress(service, &run.id, "Writing verification to server...").await;
        service
            .write_task_verification(&task.id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        let update = flowstate_core::task::UpdateTask {
            verify_status: Some(flowstate_core::task::ApprovalStatus::Pending),
            ..Default::default()
        };
        service
            .update_task(&task.id, &update)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        report_success(service, &run.id, output.exit_code).await?;
        info!("verify complete for task {}", task.id);
    } else {
        report_failure(service, &run.id, &output.stderr, output.exit_code).await?;
    }

    Ok(())
}

async fn build_prompt_context(
    service: &HttpService,
    task: &Task,
    project: &Project,
    action: ClaudeAction,
) -> PromptContext {
    // Fetch research content for downstream phases
    let research_content = if matches!(
        action,
        ClaudeAction::Design
            | ClaudeAction::Plan
            | ClaudeAction::Build
            | ClaudeAction::Verify
            | ClaudeAction::DesignDistill
            | ClaudeAction::PlanDistill
            | ClaudeAction::VerifyDistill
            | ClaudeAction::ResearchDistill
    ) {
        service.read_task_research(&task.id).await.ok()
    } else {
        None
    };

    let spec_content = if matches!(
        action,
        ClaudeAction::Plan
            | ClaudeAction::Build
            | ClaudeAction::Verify
            | ClaudeAction::PlanDistill
            | ClaudeAction::VerifyDistill
    ) {
        service.read_task_spec(&task.id).await.ok()
    } else {
        None
    };

    let plan_content = if matches!(
        action,
        ClaudeAction::Build | ClaudeAction::Verify | ClaudeAction::VerifyDistill
    ) {
        service.read_task_plan(&task.id).await.ok()
    } else {
        None
    };

    let verification_content = if matches!(action, ClaudeAction::VerifyDistill) {
        service.read_task_verification(&task.id).await.ok()
    } else {
        None
    };

    let distill_feedback = match action {
        ClaudeAction::ResearchDistill => Some(task.research_feedback.clone()),
        ClaudeAction::DesignDistill => Some(task.spec_feedback.clone()),
        ClaudeAction::PlanDistill => Some(task.plan_feedback.clone()),
        ClaudeAction::VerifyDistill => Some(task.verify_feedback.clone()),
        _ => None,
    };

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

    PromptContext {
        project_name: project.name.clone(),
        repo_url: project.repo_url.clone(),
        task_title: task.title.clone(),
        task_description: task.description.clone(),
        research_content,
        spec_content,
        plan_content,
        verification_content,
        distill_feedback,
        child_tasks,
        parent_context: None,
    }
}

fn save_prompt(run_id: &str, prompt: &str) -> Result<()> {
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

async fn progress(service: &HttpService, run_id: &str, message: &str) {
    info!("{message}");
    let _ = service.update_claude_run_progress(run_id, message).await;
}

async fn report_success(service: &HttpService, run_id: &str, exit_code: i32) -> Result<()> {
    service
        .update_claude_run_status(run_id, "completed", None, Some(exit_code))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

async fn report_failure(
    service: &HttpService,
    run_id: &str,
    error: &str,
    exit_code: i32,
) -> Result<()> {
    let msg = if error.is_empty() {
        format!("exit code {exit_code}")
    } else {
        error.to_string()
    };
    service
        .update_claude_run_status(run_id, "failed", Some(&msg), Some(exit_code))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}
