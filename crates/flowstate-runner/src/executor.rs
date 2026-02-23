use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use flowstate_core::claude_run::{ClaudeAction, ClaudeRun};
use flowstate_core::project::Project;
use flowstate_core::task::{ApprovalStatus, Task};
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
    resolve_workspace_dir_from(
        workspace_root,
        run_id,
        std::env::var("XDG_DATA_HOME").ok(),
        std::env::var_os("HOME").map(PathBuf::from),
    )
}

pub fn resolve_workspace_dir_from(
    workspace_root: &Option<PathBuf>,
    run_id: &str,
    xdg_data_home: Option<String>,
    home: Option<PathBuf>,
) -> PathBuf {
    match workspace_root {
        Some(root) => root.join(run_id),
        None => {
            if let Some(xdg) = xdg_data_home {
                PathBuf::from(xdg)
                    .join("flowstate")
                    .join("workspaces")
                    .join(run_id)
            } else if let Some(home) = home {
                home.join(".local/share/flowstate/workspaces")
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

    // Collect reviewer notes from approved prior phases for forward propagation.
    let mut reviewer_notes = Vec::new();
    if distill_feedback.is_none() {
        if task.research_status == ApprovalStatus::Approved
            && !task.research_feedback.is_empty()
            && matches!(
                action,
                ClaudeAction::Design
                    | ClaudeAction::Plan
                    | ClaudeAction::Build
                    | ClaudeAction::Verify
            )
        {
            reviewer_notes.push(("Research".to_string(), task.research_feedback.clone()));
        }
        if task.spec_status == ApprovalStatus::Approved
            && !task.spec_feedback.is_empty()
            && matches!(
                action,
                ClaudeAction::Plan | ClaudeAction::Build | ClaudeAction::Verify
            )
        {
            reviewer_notes.push(("Specification".to_string(), task.spec_feedback.clone()));
        }
        if task.plan_status == ApprovalStatus::Approved
            && !task.plan_feedback.is_empty()
            && matches!(action, ClaudeAction::Build | ClaudeAction::Verify)
        {
            reviewer_notes.push(("Plan".to_string(), task.plan_feedback.clone()));
        }
    }

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
        reviewer_notes,
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    // ── resolve_workspace_dir_from ──────────────────────────────────

    #[test]
    fn resolve_workspace_dir_from_with_explicit_root() {
        let root = PathBuf::from("/custom/workspace");
        let dir = resolve_workspace_dir_from(&Some(root.clone()), "run-42", None, None);
        assert_eq!(dir, PathBuf::from("/custom/workspace/run-42"));
    }

    #[test]
    fn resolve_workspace_dir_from_xdg_data_home() {
        let dir = resolve_workspace_dir_from(
            &None,
            "run-99",
            Some("/tmp/xdg-data".to_string()),
            Some(PathBuf::from("/home/user")),
        );
        assert_eq!(
            dir,
            PathBuf::from("/tmp/xdg-data/flowstate/workspaces/run-99")
        );
    }

    #[test]
    fn resolve_workspace_dir_from_home_fallback() {
        let dir = resolve_workspace_dir_from(
            &None,
            "run-7",
            None,
            Some(PathBuf::from("/home/testuser")),
        );
        assert_eq!(
            dir,
            PathBuf::from("/home/testuser/.local/share/flowstate/workspaces/run-7")
        );
    }

    #[test]
    fn resolve_workspace_dir_from_no_env() {
        let dir = resolve_workspace_dir_from(&None, "run-0", None, None);
        assert_eq!(dir, PathBuf::from("./flowstate/workspaces/run-0"));
    }

    #[test]
    fn resolve_workspace_dir_from_xdg_takes_priority_over_home() {
        // When both XDG_DATA_HOME and HOME are present, XDG should win
        let dir = resolve_workspace_dir_from(
            &None,
            "run-1",
            Some("/xdg-path".to_string()),
            Some(PathBuf::from("/home/user")),
        );
        assert!(
            dir.starts_with("/xdg-path"),
            "XDG_DATA_HOME should take priority over HOME: {dir:?}"
        );
    }

    #[test]
    fn resolve_workspace_dir_from_explicit_root_ignores_env() {
        // Explicit root should be used even when XDG and HOME are set
        let root = PathBuf::from("/explicit");
        let dir = resolve_workspace_dir_from(
            &Some(root),
            "run-2",
            Some("/xdg".to_string()),
            Some(PathBuf::from("/home/user")),
        );
        assert_eq!(dir, PathBuf::from("/explicit/run-2"));
    }

    // ── cleanup_workspace ───────────────────────────────────────────

    #[test]
    fn cleanup_workspace_removes_existing_dir() {
        let tmp = tempdir().unwrap();
        let ws = tmp.path().join("workspace-to-clean");
        std::fs::create_dir_all(&ws).unwrap();
        // Put a file inside to ensure recursive removal
        std::fs::write(ws.join("file.txt"), "data").unwrap();

        assert!(ws.exists());
        cleanup_workspace(&ws);
        assert!(!ws.exists());
    }

    #[test]
    fn cleanup_workspace_noop_when_missing() {
        let tmp = tempdir().unwrap();
        let ws = tmp.path().join("nonexistent-workspace");
        assert!(!ws.exists());
        // Should not panic or error
        cleanup_workspace(&ws);
        assert!(!ws.exists());
    }

    #[test]
    fn cleanup_workspace_removes_nested_dirs() {
        let tmp = tempdir().unwrap();
        let ws = tmp.path().join("deep");
        std::fs::create_dir_all(ws.join("a/b/c")).unwrap();
        std::fs::write(ws.join("a/b/c/leaf.txt"), "leaf").unwrap();

        cleanup_workspace(&ws);
        assert!(!ws.exists());
    }

    // ── save_prompt ─────────────────────────────────────────────────

    #[test]
    fn save_prompt_writes_file_with_xdg() {
        let tmp = tempdir().unwrap();
        let xdg_val = tmp.path().to_str().unwrap().to_string();

        // Temporarily override XDG_DATA_HOME for this call.
        // save_prompt reads env vars directly, so we set them around the call.
        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", &xdg_val);

        let result = save_prompt("test-run-xdg", "Hello from XDG test");

        // Restore
        match old_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }

        result.unwrap();

        let expected_path = tmp
            .path()
            .join("flowstate/claude_runs/test-run-xdg/prompt.md");
        assert!(expected_path.exists(), "prompt file should exist at {expected_path:?}");
        let content = std::fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, "Hello from XDG test");
    }

    #[test]
    fn save_prompt_writes_file_with_home_fallback() {
        let tmp = tempdir().unwrap();
        let home_val = tmp.path().to_str().unwrap().to_string();

        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        let old_home = std::env::var("HOME").ok();
        std::env::remove_var("XDG_DATA_HOME");
        std::env::set_var("HOME", &home_val);

        let result = save_prompt("test-run-home", "Hello from HOME test");

        // Restore
        match old_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }
        match old_home {
            Some(v) => std::env::set_var("HOME", v),
            None => std::env::remove_var("HOME"),
        }

        result.unwrap();

        let expected_path = tmp
            .path()
            .join(".local/share/flowstate/claude_runs/test-run-home/prompt.md");
        assert!(expected_path.exists(), "prompt file should exist at {expected_path:?}");
        let content = std::fs::read_to_string(&expected_path).unwrap();
        assert_eq!(content, "Hello from HOME test");
    }

    #[test]
    fn save_prompt_creates_directories() {
        let tmp = tempdir().unwrap();
        let xdg_val = tmp.path().to_str().unwrap().to_string();

        let old_xdg = std::env::var("XDG_DATA_HOME").ok();
        std::env::set_var("XDG_DATA_HOME", &xdg_val);

        // Directories don't exist yet - save_prompt should create them
        let result = save_prompt("brand-new-run", "prompt content");

        match old_xdg {
            Some(v) => std::env::set_var("XDG_DATA_HOME", v),
            None => std::env::remove_var("XDG_DATA_HOME"),
        }

        result.unwrap();

        let expected_dir = tmp.path().join("flowstate/claude_runs/brand-new-run");
        assert!(expected_dir.is_dir(), "run directory should be created");
    }
}
