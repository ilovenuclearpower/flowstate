//! Integration tests for workspace git operations using real temporary git repos.

use tempfile::TempDir;
use tokio::process::Command;

/// Create a bare git repo with an initial commit, returning (temp_dir, repo_url).
/// The temp_dir must be kept alive for the repo to persist.
async fn create_test_repo() -> (TempDir, String) {
    let dir = TempDir::new().unwrap();

    // Init a bare repo (to clone from)
    let bare_dir = dir.path().join("bare.git");
    std::fs::create_dir_all(&bare_dir).unwrap();
    let status = Command::new("git")
        .args(["init", "--bare"])
        .current_dir(&bare_dir)
        .status()
        .await
        .unwrap();
    assert!(status.success());

    // Clone into a work dir, add initial commit, push to bare
    let work_dir = dir.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let status = Command::new("git")
        .args(["clone", bare_dir.to_str().unwrap(), "."])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();
    assert!(status.success());

    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();

    std::fs::write(work_dir.join("README.md"), "# Test\n").unwrap();
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();
    Command::new("git")
        .args(["commit", "-m", "initial"])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();
    Command::new("git")
        .args(["push", "origin", "HEAD"])
        .current_dir(&work_dir)
        .status()
        .await
        .unwrap();

    let repo_url = bare_dir.to_str().unwrap().to_string();
    (dir, repo_url)
}

/// Helper: get the current git branch name in a directory.
async fn current_branch(dir: &std::path::Path) -> String {
    let output = Command::new("git")
        .args(["rev-parse", "--abbrev-ref", "HEAD"])
        .current_dir(dir)
        .output()
        .await
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

/// Helper: get the most recent commit message in a directory.
async fn last_commit_message(dir: &std::path::Path) -> String {
    let output = Command::new("git")
        .args(["log", "-1", "--format=%s"])
        .current_dir(dir)
        .output()
        .await
        .unwrap();
    String::from_utf8_lossy(&output.stdout).trim().to_string()
}

// ---- ensure_repo tests ----

#[tokio::test]
async fn ensure_repo_clones_local_repo() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    assert!(ws_path.join(".git").exists());
    assert!(ws_path.join("README.md").exists());
}

#[tokio::test]
async fn ensure_repo_fetches_existing_clone() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    // First clone
    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    // Second call should fetch (not fail)
    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    assert!(ws_path.join(".git").exists());
}

#[tokio::test]
async fn ensure_repo_empty_url_fails() {
    let ws = tempfile::tempdir().unwrap();
    let result = flowstate_runner::workspace::ensure_repo(ws.path(), "", None, false).await;
    assert!(result.is_err());
    let msg = result.unwrap_err().to_string();
    assert!(msg.contains("no repo_url"));
}

// ---- create_branch tests ----

#[tokio::test]
async fn create_branch_switches_to_new_branch() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    flowstate_runner::workspace::create_branch(&ws_path, "feature-1")
        .await
        .unwrap();

    let branch = current_branch(&ws_path).await;
    assert_eq!(branch, "feature-1");
}

#[tokio::test]
async fn create_branch_recreates_existing_branch() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    // Create branch first time
    flowstate_runner::workspace::create_branch(&ws_path, "feature-dup")
        .await
        .unwrap();

    // Switch back to the default branch
    Command::new("git")
        .args(["checkout", "-"])
        .current_dir(&ws_path)
        .status()
        .await
        .unwrap();

    // Create same branch again — should delete and recreate
    flowstate_runner::workspace::create_branch(&ws_path, "feature-dup")
        .await
        .unwrap();

    let branch = current_branch(&ws_path).await;
    assert_eq!(branch, "feature-dup");
}

// ---- add_and_commit tests ----

#[tokio::test]
async fn add_and_commit_with_changes() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    // Configure git user for the cloned repo
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&ws_path)
        .status()
        .await
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&ws_path)
        .status()
        .await
        .unwrap();

    flowstate_runner::workspace::create_branch(&ws_path, "test-commit")
        .await
        .unwrap();

    // Write a new file
    std::fs::write(ws_path.join("new-file.txt"), "hello world").unwrap();

    flowstate_runner::workspace::add_and_commit(&ws_path, "add new file")
        .await
        .unwrap();

    let msg = last_commit_message(&ws_path).await;
    assert_eq!(msg, "add new file");
}

#[tokio::test]
async fn add_and_commit_no_changes_succeeds() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    // Configure git user for the cloned repo
    Command::new("git")
        .args(["config", "user.email", "test@test.com"])
        .current_dir(&ws_path)
        .status()
        .await
        .unwrap();
    Command::new("git")
        .args(["config", "user.name", "Test"])
        .current_dir(&ws_path)
        .status()
        .await
        .unwrap();

    flowstate_runner::workspace::create_branch(&ws_path, "empty-commit")
        .await
        .unwrap();

    // No changes — should succeed without error
    flowstate_runner::workspace::add_and_commit(&ws_path, "nothing to commit")
        .await
        .unwrap();

    // The last commit message should still be "initial" (no new commit created)
    let msg = last_commit_message(&ws_path).await;
    assert_eq!(msg, "initial");
}

// ---- detect_default_branch tests ----

#[tokio::test]
async fn detect_default_branch_returns_main_or_master() {
    let (_dir, repo_url) = create_test_repo().await;
    let ws = tempfile::tempdir().unwrap();
    let ws_path = ws.path().join("clone");

    flowstate_runner::workspace::ensure_repo(&ws_path, &repo_url, None, false)
        .await
        .unwrap();

    let branch = flowstate_runner::workspace::detect_default_branch(&ws_path)
        .await
        .unwrap();

    // A fresh git repo's default branch is either "main" or "master"
    assert!(
        branch == "main" || branch == "master",
        "expected 'main' or 'master', got '{branch}'"
    );
}
