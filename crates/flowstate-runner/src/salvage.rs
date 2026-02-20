use std::path::Path;

use flowstate_core::claude_run::ClaudeRun;
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_service::{HttpService, TaskService};
use tracing::{info, warn, error};

use crate::config::RunnerConfig;
use crate::plan_parser;
use crate::repo_provider;
use crate::workspace;

/// Outcome of a salvage attempt.
pub enum SalvageOutcome {
    /// PR was cut from salvaged work.
    PrCut { pr_url: String, pr_number: u64 },
    /// No useful work to salvage.
    NothingToSalvage,
    /// Work existed but validation failed.
    ValidationFailed { error: String },
    /// Salvage process itself failed.
    SalvageError { error: String },
}

/// Attempt to salvage a timed-out Build run.
///
/// Strategy:
/// 1. Mark run as Salvaging
/// 2. Check if the workspace has meaningful changes (git diff)
/// 3. If changes exist:
///    a. Run validation tests
///    b. If tests pass: commit, push, cut PR -> mark Completed
///    c. If tests fail: mark Failed with salvage report
/// 4. If no changes: mark Failed ("no work completed before timeout")
pub async fn attempt_salvage(
    service: &HttpService,
    run: &ClaudeRun,
    task: &Task,
    project: &Project,
    ws_dir: &Path,
    _config: &RunnerConfig,
) -> SalvageOutcome {
    // 1. Mark run as Salvaging
    info!("salvage: starting salvage attempt for run {}", run.id);
    let _ = service
        .update_claude_run_status(&run.id, "salvaging", None, None)
        .await;
    let _ = service
        .update_claude_run_progress(&run.id, "salvage: assessing workspace...")
        .await;

    // 2. Check workspace exists
    if !ws_dir.exists() {
        warn!("salvage: workspace not found at {}", ws_dir.display());
        let _ = service
            .update_claude_run_status(
                &run.id,
                "failed",
                Some("salvage: no workspace found, run timed out before work began"),
                None,
            )
            .await;
        return SalvageOutcome::NothingToSalvage;
    }

    // 3. Check for changes via git diff --stat
    let diff_stat = match run_git_command(ws_dir, &["diff", "--stat"]).await {
        Ok(output) => output,
        Err(e) => {
            error!("salvage: failed to run git diff: {e}");
            let _ = service
                .update_claude_run_status(
                    &run.id,
                    "failed",
                    Some(&format!("salvage: git diff failed: {e}")),
                    None,
                )
                .await;
            return SalvageOutcome::SalvageError {
                error: format!("git diff failed: {e}"),
            };
        }
    };

    // Also check staged changes
    let diff_staged = run_git_command(ws_dir, &["diff", "--cached", "--stat"])
        .await
        .unwrap_or_default();

    // Check untracked files
    let untracked = run_git_command(ws_dir, &["ls-files", "--others", "--exclude-standard"])
        .await
        .unwrap_or_default();

    if diff_stat.trim().is_empty() && diff_staged.trim().is_empty() && untracked.trim().is_empty() {
        info!("salvage: no changes found in workspace");
        let _ = service
            .update_claude_run_status(
                &run.id,
                "failed",
                Some("salvage: timed out with no code changes"),
                None,
            )
            .await;
        return SalvageOutcome::NothingToSalvage;
    }

    info!("salvage: workspace has changes, proceeding with validation");
    let _ = service
        .update_claude_run_progress(&run.id, "salvage: running validation...")
        .await;

    // 4. Extract validation commands from plan
    let plan_content = service.read_task_plan(&task.id).await.ok();
    let validation_steps = plan_content
        .as_deref()
        .map(plan_parser::extract_validation_commands)
        .unwrap_or_default();

    // 5. Run validation if steps exist
    if !validation_steps.is_empty() {
        info!(
            "salvage: running {} validation steps",
            validation_steps.len()
        );
        let _ = service
            .update_claude_run_progress(
                &run.id,
                &format!(
                    "salvage: running validation ({} steps)...",
                    validation_steps.len()
                ),
            )
            .await;

        let verifier = flowstate_verify::Runner::new();
        let result = verifier.execute(&validation_steps, ws_dir).await;

        match result.status {
            flowstate_verify::runner::RunStatus::Passed => {
                info!("salvage: all validation steps passed");
            }
            _ => {
                // Validation failed
                let mut error_msg = String::from("salvage: validation failed after timeout:\n");
                for step in &result.steps {
                    if step.exit_code != Some(0) {
                        error_msg.push_str(&format!(
                            "\n--- {} (exit {}) ---\n{}\n{}\n",
                            step.step_name,
                            step.exit_code
                                .map_or("timeout".to_string(), |c| c.to_string()),
                            step.stdout,
                            step.stderr,
                        ));
                    }
                }
                error!("salvage: validation failed, marking run as failed");
                let _ = service
                    .update_claude_run_status(&run.id, "failed", Some(&error_msg), Some(1))
                    .await;
                return SalvageOutcome::ValidationFailed { error: error_msg };
            }
        }
    }

    // 6. Tests pass (or no tests) — commit, push, cut PR
    let _ = service
        .update_claude_run_progress(&run.id, "salvage: committing changes...")
        .await;

    // Stage and commit
    let commit_msg = format!("feat: {} [flowstate] [salvaged]", task.title);
    if let Err(e) = workspace::add_and_commit(ws_dir, &commit_msg).await {
        error!("salvage: commit failed: {e}");
        let _ = service
            .update_claude_run_status(
                &run.id,
                "failed",
                Some(&format!("salvage: commit failed: {e}")),
                None,
            )
            .await;
        return SalvageOutcome::SalvageError {
            error: format!("commit failed: {e}"),
        };
    }

    // Resolve provider and push
    let provider = match repo_provider::provider_for_url(&project.repo_url) {
        Ok(p) => p,
        Err(e) => {
            error!("salvage: unsupported repo provider: {e}");
            let _ = service
                .update_claude_run_status(
                    &run.id,
                    "failed",
                    Some(&format!("salvage: unsupported repo provider: {e}")),
                    None,
                )
                .await;
            return SalvageOutcome::SalvageError {
                error: format!("unsupported repo provider: {e}"),
            };
        }
    };

    // Determine branch name from any existing branch or create a salvage branch
    let branch_name = get_current_branch(ws_dir)
        .await
        .unwrap_or_else(|| format!("flowstate/salvage-{}", &run.id[..8]));

    let _ = service
        .update_claude_run_progress(&run.id, "salvage: pushing branch...")
        .await;

    if let Err(e) = provider.push_branch(ws_dir, &branch_name).await {
        // Try force-with-lease in case branch partially pushed before timeout
        warn!("salvage: push failed ({e}), trying force-with-lease...");
        let force_result = run_git_command(
            ws_dir,
            &["push", "-u", "origin", &branch_name, "--force-with-lease"],
        )
        .await;
        if let Err(e2) = force_result {
            error!("salvage: force push also failed: {e2}");
            let _ = service
                .update_claude_run_status(
                    &run.id,
                    "failed",
                    Some(&format!("salvage: push failed: {e}")),
                    None,
                )
                .await;
            return SalvageOutcome::SalvageError {
                error: format!("push failed: {e}"),
            };
        }
    }

    // Cut PR
    let _ = service
        .update_claude_run_progress(&run.id, "salvage: opening pull request...")
        .await;

    let default_branch = workspace::detect_default_branch(ws_dir)
        .await
        .unwrap_or_else(|_| "main".to_string());

    let pr_body = format!(
        "## Task\n\n{}\n\n## Description\n\n{}\n\n---\n**Note:** This PR was salvaged from a timed-out build run.\n\nGenerated by flowstate runner",
        task.title, task.description
    );

    match provider
        .open_pull_request(ws_dir, &branch_name, &task.title, &pr_body, &default_branch)
        .await
    {
        Ok(pr) => {
            info!(
                "salvage: PR #{} created at {}",
                pr.number, pr.url
            );

            // Update run with PR info
            let _ = service
                .update_claude_run_pr(
                    &run.id,
                    Some(&pr.url),
                    Some(pr.number as i64),
                    Some(&pr.branch),
                )
                .await;

            // Link PR to task
            let create_pr = flowstate_core::task_pr::CreateTaskPr {
                task_id: task.id.clone(),
                claude_run_id: Some(run.id.clone()),
                pr_url: pr.url.clone(),
                pr_number: pr.number as i64,
                branch_name: pr.branch.clone(),
            };
            let _ = service.create_task_pr(&create_pr).await;

            // Mark completed
            let _ = service
                .update_claude_run_status(&run.id, "completed", None, None)
                .await;
            let _ = service
                .update_claude_run_progress(&run.id, "salvaged after timeout")
                .await;

            SalvageOutcome::PrCut {
                pr_url: pr.url,
                pr_number: pr.number,
            }
        }
        Err(e) => {
            error!("salvage: PR creation failed: {e}");
            let _ = service
                .update_claude_run_status(
                    &run.id,
                    "failed",
                    Some(&format!("salvage: PR creation failed: {e}")),
                    None,
                )
                .await;
            SalvageOutcome::SalvageError {
                error: format!("PR creation failed: {e}"),
            }
        }
    }
}

/// Run a git command in the workspace and return stdout as a string.
async fn run_git_command(dir: &Path, args: &[&str]) -> Result<String, String> {
    let output = tokio::process::Command::new("git")
        .args(args)
        .current_dir(dir)
        .output()
        .await
        .map_err(|e| format!("spawn git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!(
            "git {} failed: {stderr}",
            args.join(" ")
        ))
    }
}

/// Get the current git branch name.
async fn get_current_branch(dir: &Path) -> Option<String> {
    let output = tokio::process::Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .await
        .ok()?;

    if output.status.success() {
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if branch.is_empty() || branch == "HEAD" {
            None
        } else {
            Some(branch)
        }
    } else {
        None
    }
}
