use std::sync::Arc;

use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_db::Database;
use flowstate_prompts::{ChildTaskInfo, PromptContext};
use std::process::Stdio;
use tokio::process::Command;

/// Execute a Claude Code run asynchronously.
///
/// This is spawned via `tokio::spawn` from the route handler.
/// It assembles a prompt, invokes `claude -p`, captures output,
/// and updates the DB record.
pub async fn execute_run(
    db: Arc<dyn Database>,
    run_id: String,
    task: Task,
    project: Project,
    action: ClaudeAction,
) {
    // Mark as running
    if let Err(e) = db.update_claude_run_status(&run_id, ClaudeRunStatus::Running, None, None).await {
        eprintln!("claude-runner: failed to mark run {run_id} as running: {e}");
        return;
    }

    let prompt = build_prompt(&*db, &task, &project, action).await;

    // Set up output directory
    let run_dir = flowstate_db::claude_run_dir(&run_id);
    if let Err(e) = std::fs::create_dir_all(&run_dir) {
        eprintln!("claude-runner: failed to create run dir: {e}");
        let _ = db.update_claude_run_status(
            &run_id,
            ClaudeRunStatus::Failed,
            Some(&format!("create run dir: {e}")),
            None,
        ).await;
        return;
    }

    // Save prompt for debugging
    let prompt_path = run_dir.join("prompt.md");
    let _ = std::fs::write(&prompt_path, &prompt);

    // Determine working directory
    let work_dir = match action {
        ClaudeAction::Build => {
            // For build, use workspace directory (git clone)
            let ws = flowstate_db::workspace_dir(&project.id);
            if let Err(e) = ensure_workspace(&ws, &project.repo_url).await {
                let _ = db.update_claude_run_status(
                    &run_id,
                    ClaudeRunStatus::Failed,
                    Some(&format!("workspace setup: {e}")),
                    None,
                ).await;
                return;
            }
            ws
        }
        _ => {
            // For design/plan, work in the task directory
            let td = flowstate_db::task_dir(&task.id);
            let _ = std::fs::create_dir_all(&td);
            td
        }
    };

    // Spawn claude -p
    let result = Command::new("claude")
        .arg("-p")
        .arg(&prompt)
        .arg("--output-format")
        .arg("text")
        .current_dir(&work_dir)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .await;

    match result {
        Ok(output) => {
            let stdout = String::from_utf8_lossy(&output.stdout).to_string();
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            let exit_code = output.status.code().unwrap_or(-1);

            // Save output
            let output_path = run_dir.join("output.txt");
            let _ = std::fs::write(&output_path, &stdout);

            if output.status.success() {
                // On success, pick up the named output file and copy to canonical location
                match action {
                    ClaudeAction::Design => {
                        let written = work_dir.join("SPECIFICATION.md");
                        let content = std::fs::read_to_string(&written)
                            .unwrap_or_else(|_| stdout.clone());
                        let spec_path = flowstate_db::task_spec_path(&task.id);
                        let _ = std::fs::create_dir_all(spec_path.parent().unwrap());
                        let _ = std::fs::write(&spec_path, &content);
                        // Clean up the working copy
                        let _ = std::fs::remove_file(&written);
                        // Set spec_status = pending
                        let update = flowstate_core::task::UpdateTask {
                            spec_status: Some(flowstate_core::task::ApprovalStatus::Pending),
                            ..Default::default()
                        };
                        let _ = db.update_task(&task.id, &update).await;
                    }
                    ClaudeAction::Plan => {
                        let written = work_dir.join("PLAN.md");
                        let content = std::fs::read_to_string(&written)
                            .unwrap_or_else(|_| stdout.clone());
                        let plan_path = flowstate_db::task_plan_path(&task.id);
                        let _ = std::fs::create_dir_all(plan_path.parent().unwrap());
                        let _ = std::fs::write(&plan_path, &content);
                        let _ = std::fs::remove_file(&written);
                        // Set plan_status = pending
                        let update = flowstate_core::task::UpdateTask {
                            plan_status: Some(flowstate_core::task::ApprovalStatus::Pending),
                            ..Default::default()
                        };
                        let _ = db.update_task(&task.id, &update).await;
                    }
                    ClaudeAction::Build => {
                        // Build output is just logged, no file copy needed
                    }
                    _ => {
                        // Other actions (Research, Verify, distill variants)
                        // are handled by the runner, not the server executor
                    }
                }

                let _ = db.update_claude_run_status(
                    &run_id,
                    ClaudeRunStatus::Completed,
                    None,
                    Some(exit_code),
                ).await;
            } else {
                let error_msg = if stderr.is_empty() {
                    format!("exit code {exit_code}")
                } else {
                    stderr
                };
                let _ = db.update_claude_run_status(
                    &run_id,
                    ClaudeRunStatus::Failed,
                    Some(&error_msg),
                    Some(exit_code),
                ).await;
            }
        }
        Err(e) => {
            let _ = db.update_claude_run_status(
                &run_id,
                ClaudeRunStatus::Failed,
                Some(&format!("spawn claude: {e}")),
                None,
            ).await;
        }
    }
}

/// Build the prompt using flowstate-prompts, reading spec/plan/children from the DB/filesystem.
async fn build_prompt(db: &dyn Database, task: &Task, project: &Project, action: ClaudeAction) -> String {
    let spec_content = if matches!(action, ClaudeAction::Plan | ClaudeAction::Build) {
        let spec_path = flowstate_db::task_spec_path(&task.id);
        std::fs::read_to_string(&spec_path).ok()
    } else {
        None
    };

    let plan_content = if matches!(action, ClaudeAction::Build) {
        let plan_path = flowstate_db::task_plan_path(&task.id);
        std::fs::read_to_string(&plan_path).ok()
    } else {
        None
    };

    let child_tasks = db
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
        plan_content,
        research_content: None,
        verification_content: None,
        distill_feedback: None,
        child_tasks,
    };

    flowstate_prompts::assemble_prompt(&ctx, action)
}

/// Ensure the workspace directory exists with a git clone/pull.
async fn ensure_workspace(
    workspace: &std::path::Path,
    repo_url: &str,
) -> Result<(), String> {
    if repo_url.is_empty() {
        return Err("project has no repo_url configured".into());
    }

    if workspace.join(".git").exists() {
        // Pull latest
        let output = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(workspace)
            .output()
            .await
            .map_err(|e| format!("git pull: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git pull failed: {stderr}"));
        }
    } else {
        // Clone
        std::fs::create_dir_all(workspace).map_err(|e| format!("mkdir: {e}"))?;
        let output = Command::new("git")
            .args(["clone", repo_url, "."])
            .current_dir(workspace)
            .output()
            .await
            .map_err(|e| format!("git clone: {e}"))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(format!("git clone failed: {stderr}"));
        }
    }

    Ok(())
}
