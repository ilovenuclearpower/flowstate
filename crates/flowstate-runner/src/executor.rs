use std::path::PathBuf;
use std::process::Stdio;

use anyhow::{Context, Result};
use flowstate_core::claude_run::{ClaudeAction, ClaudeRun};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_prompts::{ChildTaskInfo, PromptContext};
use flowstate_service::{HttpService, TaskService};
use tokio::process::Command;
use tracing::info;

use crate::pipeline;

/// Dispatch a claimed run to the appropriate handler.
pub async fn dispatch(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    workspace_root: &Option<PathBuf>,
) -> Result<()> {
    match run.action {
        ClaudeAction::Design => execute_design(service, run, task, project).await,
        ClaudeAction::Plan => execute_plan(service, run, task, project).await,
        ClaudeAction::Build => {
            pipeline::execute(service, run, task, project, workspace_root).await
        }
    }
}

async fn execute_design(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
) -> Result<()> {
    let ctx = build_prompt_context(service, task, project, ClaudeAction::Design).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, ClaudeAction::Design);

    // Work dir: a temp-ish directory for the task
    let work_dir = task_work_dir(&task.id);
    std::fs::create_dir_all(&work_dir)?;

    save_prompt(&run.id, &prompt)?;

    let output = run_claude(&prompt, &work_dir).await?;

    if output.success {
        // Read SPECIFICATION.md from work dir
        let spec_file = work_dir.join("SPECIFICATION.md");
        let content = std::fs::read_to_string(&spec_file)
            .unwrap_or_else(|_| output.stdout.clone());

        // Write spec via HTTP
        service
            .write_task_spec(&task.id, &content)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Set spec_status = pending
        let update = flowstate_core::task::UpdateTask {
            spec_status: Some(flowstate_core::task::ApprovalStatus::Pending),
            ..Default::default()
        };
        service
            .update_task(&task.id, &update)
            .await
            .map_err(|e| anyhow::anyhow!("{e}"))?;

        // Clean up
        let _ = std::fs::remove_file(&spec_file);

        report_success(service, &run.id, output.exit_code).await?;
        info!("design complete for task {}", task.id);
    } else {
        report_failure(service, &run.id, &output.stderr, output.exit_code).await?;
    }

    Ok(())
}

async fn execute_plan(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
) -> Result<()> {
    let ctx = build_prompt_context(service, task, project, ClaudeAction::Plan).await;
    let prompt = flowstate_prompts::assemble_prompt(&ctx, ClaudeAction::Plan);

    let work_dir = task_work_dir(&task.id);
    std::fs::create_dir_all(&work_dir)?;

    save_prompt(&run.id, &prompt)?;

    let output = run_claude(&prompt, &work_dir).await?;

    if output.success {
        let plan_file = work_dir.join("PLAN.md");
        let content = std::fs::read_to_string(&plan_file)
            .unwrap_or_else(|_| output.stdout.clone());

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

        let _ = std::fs::remove_file(&plan_file);

        report_success(service, &run.id, output.exit_code).await?;
        info!("plan complete for task {}", task.id);
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
    let spec_content = if matches!(action, ClaudeAction::Plan | ClaudeAction::Build) {
        service.read_task_spec(&task.id).await.ok()
    } else {
        None
    };

    let plan_content = if matches!(action, ClaudeAction::Build) {
        service.read_task_plan(&task.id).await.ok()
    } else {
        None
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
        spec_content,
        plan_content,
        child_tasks,
    }
}

pub struct ClaudeOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

pub async fn run_claude(prompt: &str, work_dir: &std::path::Path) -> Result<ClaudeOutput> {
    let result = Command::new("claude")
        .arg("-p")
        .arg(prompt)
        .arg("--output-format")
        .arg("text")
        .current_dir(work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await
        .context("spawn claude")?;

    let stdout = String::from_utf8_lossy(&result.stdout).to_string();
    let stderr = String::from_utf8_lossy(&result.stderr).to_string();
    let exit_code = result.status.code().unwrap_or(-1);

    // Save output
    let run_dir = work_dir.join(".flowstate-output");
    let _ = std::fs::create_dir_all(&run_dir);
    let _ = std::fs::write(run_dir.join("output.txt"), &stdout);

    Ok(ClaudeOutput {
        success: result.status.success(),
        stdout,
        stderr,
        exit_code,
    })
}

fn task_work_dir(task_id: &str) -> PathBuf {
    flowstate_db_data_dir().join("tasks").join(task_id)
}

fn flowstate_db_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg).join("flowstate")
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home)
            .join(".local/share")
            .join("flowstate")
    } else {
        PathBuf::from(".").join("flowstate")
    }
}

fn save_prompt(run_id: &str, prompt: &str) -> Result<()> {
    let run_dir = flowstate_db_data_dir()
        .join("claude_runs")
        .join(run_id);
    std::fs::create_dir_all(&run_dir)?;
    std::fs::write(run_dir.join("prompt.md"), prompt)?;
    Ok(())
}

async fn report_success(service: &HttpService, run_id: &str, exit_code: i32) -> Result<()> {
    service
        .update_claude_run_status(run_id, "completed", None, Some(exit_code))
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))?;
    Ok(())
}

async fn report_failure(service: &HttpService, run_id: &str, error: &str, exit_code: i32) -> Result<()> {
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
