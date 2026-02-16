use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus};
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_db::Db;
use std::process::Stdio;
use tokio::process::Command;

/// Execute a Claude Code run asynchronously.
///
/// This is spawned via `tokio::spawn` from the route handler.
/// It assembles a prompt, invokes `claude -p`, captures output,
/// and updates the DB record.
pub async fn execute_run(
    db: Db,
    run_id: String,
    task: Task,
    project: Project,
    action: ClaudeAction,
) {
    // Mark as running
    if let Err(e) = db.update_claude_run_status(&run_id, ClaudeRunStatus::Running, None, None) {
        eprintln!("claude-runner: failed to mark run {run_id} as running: {e}");
        return;
    }

    let prompt = assemble_prompt(&db, &task, &project, action);

    // Set up output directory
    let run_dir = flowstate_db::claude_run_dir(&run_id);
    if let Err(e) = std::fs::create_dir_all(&run_dir) {
        eprintln!("claude-runner: failed to create run dir: {e}");
        let _ = db.update_claude_run_status(
            &run_id,
            ClaudeRunStatus::Failed,
            Some(&format!("create run dir: {e}")),
            None,
        );
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
                );
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
                // On success, copy output to appropriate file
                match action {
                    ClaudeAction::Design => {
                        let spec_path = flowstate_db::task_spec_path(&task.id);
                        let _ = std::fs::create_dir_all(spec_path.parent().unwrap());
                        let _ = std::fs::write(&spec_path, &stdout);
                        // Set spec_status = pending
                        let update = flowstate_core::task::UpdateTask {
                            spec_status: Some(flowstate_core::task::ApprovalStatus::Pending),
                            ..Default::default()
                        };
                        let _ = db.update_task(&task.id, &update);
                    }
                    ClaudeAction::Plan => {
                        let plan_path = flowstate_db::task_plan_path(&task.id);
                        let _ = std::fs::create_dir_all(plan_path.parent().unwrap());
                        let _ = std::fs::write(&plan_path, &stdout);
                        // Set plan_status = pending
                        let update = flowstate_core::task::UpdateTask {
                            plan_status: Some(flowstate_core::task::ApprovalStatus::Pending),
                            ..Default::default()
                        };
                        let _ = db.update_task(&task.id, &update);
                    }
                    ClaudeAction::Build => {
                        // Build output is just logged, no file copy needed
                    }
                }

                let _ = db.update_claude_run_status(
                    &run_id,
                    ClaudeRunStatus::Completed,
                    None,
                    Some(exit_code),
                );
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
                );
            }
        }
        Err(e) => {
            let _ = db.update_claude_run_status(
                &run_id,
                ClaudeRunStatus::Failed,
                Some(&format!("spawn claude: {e}")),
                None,
            );
        }
    }
}

fn assemble_prompt(db: &Db, task: &Task, project: &Project, action: ClaudeAction) -> String {
    let mut prompt = String::new();

    prompt.push_str(&format!("# Project: {}\n\n", project.name));
    if !project.repo_url.is_empty() {
        prompt.push_str(&format!("Repository: {}\n\n", project.repo_url));
    }

    prompt.push_str(&format!("# Task: {}\n\n", task.title));
    prompt.push_str(&format!("## Description\n\n{}\n\n", task.description));

    // Include spec if it exists (for plan and build)
    if matches!(action, ClaudeAction::Plan | ClaudeAction::Build) {
        let spec_path = flowstate_db::task_spec_path(&task.id);
        if let Ok(spec) = std::fs::read_to_string(&spec_path) {
            prompt.push_str("## Specification\n\n");
            prompt.push_str(&spec);
            prompt.push_str("\n\n");
        }
    }

    // Include plan if it exists (for build)
    if matches!(action, ClaudeAction::Build) {
        let plan_path = flowstate_db::task_plan_path(&task.id);
        if let Ok(plan) = std::fs::read_to_string(&plan_path) {
            prompt.push_str("## Implementation Plan\n\n");
            prompt.push_str(&plan);
            prompt.push_str("\n\n");
        }
    }

    // Include child tasks for context
    if let Ok(children) = db.list_child_tasks(&task.id) {
        if !children.is_empty() {
            prompt.push_str("## Sub-tasks\n\n");
            for child in &children {
                prompt.push_str(&format!(
                    "- [{}] {}: {}\n",
                    child.status.as_str(),
                    child.title,
                    child.description
                ));
            }
            prompt.push_str("\n");
        }
    }

    // Action-specific instructions
    match action {
        ClaudeAction::Design => {
            prompt.push_str("## Instructions\n\n");
            prompt.push_str(
                "Produce a detailed technical specification for this task. \
                 The specification should include:\n\
                 - Problem statement and goals\n\
                 - Proposed solution architecture\n\
                 - API changes or new interfaces\n\
                 - Data model changes\n\
                 - Edge cases and error handling\n\
                 - Testing strategy\n\n\
                 Output the specification as a well-structured markdown document.\n",
            );
        }
        ClaudeAction::Plan => {
            prompt.push_str("## Instructions\n\n");
            prompt.push_str(
                "Based on the specification above, produce a detailed implementation plan. \
                 The plan should include:\n\
                 - Step-by-step implementation order\n\
                 - Files to create or modify\n\
                 - Key code changes for each step\n\
                 - Dependencies between steps\n\
                 - Verification steps (tests to run, manual checks)\n\n\
                 Output the plan as a well-structured markdown document.\n",
            );
        }
        ClaudeAction::Build => {
            prompt.push_str("## Instructions\n\n");
            prompt.push_str(
                "Implement the changes described in the specification and plan above. \
                 Follow the implementation plan step by step. \
                 Write clean, well-tested code. \
                 Ensure all existing tests continue to pass.\n",
            );
        }
    }

    prompt
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
