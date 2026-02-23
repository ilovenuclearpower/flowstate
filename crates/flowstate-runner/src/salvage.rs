use std::path::Path;

use flowstate_core::claude_run::ClaudeRun;
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_service::{HttpService, TaskService};
use tracing::{error, info, warn};

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
    let diff_stat = match run_git_command(ws_dir, &["diff", "--stat"], false).await {
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
    let diff_staged = run_git_command(ws_dir, &["diff", "--cached", "--stat"], false)
        .await
        .unwrap_or_default();

    // Check untracked files
    let untracked = run_git_command(
        ws_dir,
        &["ls-files", "--others", "--exclude-standard"],
        false,
    )
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

    // Resolve provider and push (with PAT for gh CLI auth)
    let token = service.get_repo_token(&project.id).await.ok();
    let provider = match repo_provider::provider_for_url(
        &project.repo_url,
        token,
        project.provider_type,
        project.skip_tls_verify,
    ) {
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
            project.skip_tls_verify,
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
            info!("salvage: PR #{} created at {}", pr.number, pr.url);

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
async fn run_git_command(
    dir: &Path,
    args: &[&str],
    skip_tls_verify: bool,
) -> Result<String, String> {
    let mut cmd = tokio::process::Command::new("git");
    cmd.args(args).current_dir(dir);
    if skip_tls_verify {
        cmd.env("GIT_SSL_NO_VERIFY", "true");
    }

    let output = cmd.output().await.map_err(|e| format!("spawn git: {e}"))?;

    if output.status.success() {
        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    } else {
        let stderr = String::from_utf8_lossy(&output.stderr);
        Err(format!("git {} failed: {stderr}", args.join(" ")))
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

#[cfg(test)]
mod tests {
    use super::*;

    /// Helper: create a temp dir with an initialized git repo and one commit.
    async fn init_git_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        let dir = tmp.path().to_path_buf();

        // git init
        tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .await
            .expect("git init failed");

        // configure user
        tokio::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&dir)
            .output()
            .await
            .expect("git config email failed");

        tokio::process::Command::new("git")
            .args(["config", "user.name", "test"])
            .current_dir(&dir)
            .output()
            .await
            .expect("git config name failed");

        // create a file and commit so HEAD exists
        tokio::fs::write(dir.join("README.md"), "hello")
            .await
            .unwrap();

        tokio::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .await
            .expect("git add failed");

        tokio::process::Command::new("git")
            .args(["commit", "-m", "initial commit"])
            .current_dir(&dir)
            .output()
            .await
            .expect("git commit failed");

        (tmp, dir)
    }

    // ---- run_git_command tests ----

    #[tokio::test]
    async fn test_run_git_command_success() {
        let (_tmp, dir) = init_git_repo().await;
        let result = run_git_command(&dir, &["status"], false).await;
        assert!(result.is_ok(), "expected Ok, got: {result:?}");
        let stdout = result.unwrap();
        // `git status` in a clean repo should mention "nothing to commit"
        // or "on branch" — either way it should not be empty.
        assert!(!stdout.is_empty());
    }

    #[tokio::test]
    async fn test_run_git_command_failure() {
        let (_tmp, dir) = init_git_repo().await;
        // "git log --invalid-flag-xyz" should fail
        let result = run_git_command(&dir, &["log", "--invalid-flag-xyz"], false).await;
        assert!(result.is_err(), "expected Err for invalid git flag");
        let err = result.unwrap_err();
        assert!(
            err.contains("git log --invalid-flag-xyz failed"),
            "error should describe the failing command, got: {err}"
        );
    }

    #[tokio::test]
    async fn test_run_git_command_nonexistent_dir() {
        let dir = Path::new("/tmp/flowstate_nonexistent_dir_for_test");
        let result = run_git_command(dir, &["status"], false).await;
        // Either spawn error or git error — both should be Err
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_run_git_command_skip_tls_verify() {
        let (_tmp, dir) = init_git_repo().await;
        // With skip_tls_verify=true, the command should still succeed for local ops
        let result = run_git_command(&dir, &["status"], true).await;
        assert!(
            result.is_ok(),
            "expected Ok with skip_tls_verify, got: {result:?}"
        );
    }

    #[tokio::test]
    async fn test_run_git_command_diff_stat_empty() {
        let (_tmp, dir) = init_git_repo().await;
        // No changes, so diff --stat should return empty string
        let result = run_git_command(&dir, &["diff", "--stat"], false).await;
        assert!(result.is_ok());
        assert!(result.unwrap().trim().is_empty());
    }

    #[tokio::test]
    async fn test_run_git_command_diff_stat_with_changes() {
        let (_tmp, dir) = init_git_repo().await;
        // Modify the file so diff --stat has output
        tokio::fs::write(dir.join("README.md"), "modified content")
            .await
            .unwrap();
        let result = run_git_command(&dir, &["diff", "--stat"], false).await;
        assert!(result.is_ok());
        let output = result.unwrap();
        assert!(!output.trim().is_empty(), "diff --stat should show changes");
        assert!(output.contains("README.md"));
    }

    // ---- get_current_branch tests ----

    #[tokio::test]
    async fn test_get_current_branch_on_repo() {
        let (_tmp, dir) = init_git_repo().await;
        let branch = get_current_branch(&dir).await;
        assert!(branch.is_some(), "expected Some branch name");
        // Default branch after git init is typically "main" or "master"
        let name = branch.unwrap();
        assert!(
            name == "main" || name == "master",
            "expected main or master, got: {name}"
        );
    }

    #[tokio::test]
    async fn test_get_current_branch_custom_branch() {
        let (_tmp, dir) = init_git_repo().await;
        // Create and checkout a feature branch
        tokio::process::Command::new("git")
            .args(["checkout", "-b", "flowstate/test-branch"])
            .current_dir(&dir)
            .output()
            .await
            .expect("git checkout -b failed");

        let branch = get_current_branch(&dir).await;
        assert_eq!(branch, Some("flowstate/test-branch".to_string()));
    }

    #[tokio::test]
    async fn test_get_current_branch_non_git_dir() {
        let tmp = tempfile::tempdir().expect("failed to create tempdir");
        // This directory is not a git repo
        let branch = get_current_branch(tmp.path()).await;
        assert!(branch.is_none(), "expected None for non-git directory");
    }

    #[tokio::test]
    async fn test_get_current_branch_nonexistent_dir() {
        let dir = Path::new("/tmp/flowstate_nonexistent_dir_for_branch_test");
        let branch = get_current_branch(dir).await;
        assert!(branch.is_none(), "expected None for nonexistent directory");
    }

    // ---- SalvageOutcome tests ----

    #[test]
    fn test_salvage_outcome_pr_cut_variant() {
        let outcome = SalvageOutcome::PrCut {
            pr_url: "https://github.com/org/repo/pull/42".to_string(),
            pr_number: 42,
        };
        assert!(matches!(
            outcome,
            SalvageOutcome::PrCut { pr_number: 42, .. }
        ));
    }

    #[test]
    fn test_salvage_outcome_nothing_to_salvage_variant() {
        let outcome = SalvageOutcome::NothingToSalvage;
        assert!(matches!(outcome, SalvageOutcome::NothingToSalvage));
    }

    #[test]
    fn test_salvage_outcome_validation_failed_variant() {
        let outcome = SalvageOutcome::ValidationFailed {
            error: "test failure".to_string(),
        };
        assert!(matches!(outcome, SalvageOutcome::ValidationFailed { .. }));
    }

    #[test]
    fn test_salvage_outcome_salvage_error_variant() {
        let outcome = SalvageOutcome::SalvageError {
            error: "something went wrong".to_string(),
        };
        assert!(matches!(outcome, SalvageOutcome::SalvageError { .. }));
    }

    #[test]
    fn test_salvage_outcome_pr_cut_fields() {
        let outcome = SalvageOutcome::PrCut {
            pr_url: "https://example.com/pr/1".to_string(),
            pr_number: 1,
        };
        if let SalvageOutcome::PrCut { pr_url, pr_number } = outcome {
            assert_eq!(pr_url, "https://example.com/pr/1");
            assert_eq!(pr_number, 1);
        } else {
            panic!("expected PrCut variant");
        }
    }

    #[test]
    fn test_salvage_outcome_validation_failed_field() {
        let outcome = SalvageOutcome::ValidationFailed {
            error: "cargo test failed".to_string(),
        };
        if let SalvageOutcome::ValidationFailed { error } = outcome {
            assert_eq!(error, "cargo test failed");
        } else {
            panic!("expected ValidationFailed variant");
        }
    }

    #[test]
    fn test_salvage_outcome_salvage_error_field() {
        let outcome = SalvageOutcome::SalvageError {
            error: "push failed".to_string(),
        };
        if let SalvageOutcome::SalvageError { error } = outcome {
            assert_eq!(error, "push failed");
        } else {
            panic!("expected SalvageError variant");
        }
    }
}
