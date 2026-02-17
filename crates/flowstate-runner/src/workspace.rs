use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::info;

/// Ensure the workspace directory exists with a git clone/pull.
pub async fn ensure_repo(workspace: &Path, repo_url: &str) -> Result<()> {
    if repo_url.is_empty() {
        bail!("project has no repo_url configured");
    }

    if workspace.join(".git").exists() {
        // Pull latest on the default branch
        let output = Command::new("git")
            .args(["pull", "--ff-only"])
            .current_dir(workspace)
            .output()
            .await
            .context("git pull")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git pull failed: {stderr}");
        }
        info!("workspace updated via git pull");
    } else {
        std::fs::create_dir_all(workspace).context("create workspace dir")?;
        let output = Command::new("git")
            .args(["clone", repo_url, "."])
            .current_dir(workspace)
            .output()
            .await
            .context("git clone")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git clone failed: {stderr}");
        }
        info!("workspace cloned from {repo_url}");
    }

    Ok(())
}

/// Create and switch to a new branch.
pub async fn create_branch(dir: &Path, name: &str) -> Result<()> {
    let output = Command::new("git")
        .args(["checkout", "-b", name])
        .current_dir(dir)
        .output()
        .await
        .context("git checkout -b")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git checkout -b {name} failed: {stderr}");
    }
    info!("created branch {name}");
    Ok(())
}

/// Stage all changes and commit.
pub async fn add_and_commit(dir: &Path, message: &str) -> Result<()> {
    let add = Command::new("git")
        .args(["add", "-A"])
        .current_dir(dir)
        .output()
        .await
        .context("git add -A")?;

    if !add.status.success() {
        let stderr = String::from_utf8_lossy(&add.stderr);
        bail!("git add -A failed: {stderr}");
    }

    // Check if there's anything to commit
    let status = Command::new("git")
        .args(["status", "--porcelain"])
        .current_dir(dir)
        .output()
        .await
        .context("git status")?;

    if status.stdout.is_empty() {
        info!("nothing to commit, working tree clean");
        return Ok(());
    }

    let commit = Command::new("git")
        .args(["commit", "-m", message])
        .current_dir(dir)
        .output()
        .await
        .context("git commit")?;

    if !commit.status.success() {
        let stderr = String::from_utf8_lossy(&commit.stderr);
        bail!("git commit failed: {stderr}");
    }
    info!("committed: {message}");
    Ok(())
}

/// Detect the default branch (main/master) from the remote.
pub async fn detect_default_branch(dir: &Path) -> Result<String> {
    // Try symbolic-ref first
    let output = Command::new("git")
        .args(["symbolic-ref", "refs/remotes/origin/HEAD"])
        .current_dir(dir)
        .output()
        .await;

    if let Ok(ref out) = output {
        if out.status.success() {
            let full_ref = String::from_utf8_lossy(&out.stdout).trim().to_string();
            // refs/remotes/origin/main → main
            if let Some(branch) = full_ref.strip_prefix("refs/remotes/origin/") {
                return Ok(branch.to_string());
            }
        }
    }

    // Fallback: check if 'main' exists
    let check_main = Command::new("git")
        .args(["rev-parse", "--verify", "origin/main"])
        .current_dir(dir)
        .output()
        .await;

    if let Ok(ref out) = check_main {
        if out.status.success() {
            return Ok("main".to_string());
        }
    }

    // Last resort
    Ok("master".to_string())
}

/// Ensure we're on the default branch and up to date before creating a feature branch.
pub async fn checkout_default_branch(dir: &Path) -> Result<String> {
    let default = detect_default_branch(dir).await?;

    let output = Command::new("git")
        .args(["checkout", &default])
        .current_dir(dir)
        .output()
        .await
        .context("git checkout default branch")?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!("git checkout {default} failed: {stderr}");
    }

    Ok(default)
}
