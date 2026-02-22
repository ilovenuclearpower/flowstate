use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::info;

/// Ensure the workspace directory exists with a git clone/pull.
pub async fn ensure_repo(
    workspace: &Path,
    repo_url: &str,
    repo_token: Option<&str>,
) -> Result<()> {
    if repo_url.is_empty() {
        bail!("project has no repo_url configured");
    }

    let auth_url = inject_token(repo_url, repo_token);

    if workspace.join(".git").exists() {
        // Update the remote URL in case token changed
        let _ = Command::new("git")
            .args(["remote", "set-url", "origin", &auth_url])
            .current_dir(workspace)
            .output()
            .await;

        // Fetch latest from origin (don't pull — we may be on a branch without tracking)
        let output = Command::new("git")
            .args(["fetch", "origin"])
            .current_dir(workspace)
            .output()
            .await
            .context("git fetch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git fetch failed: {stderr}");
        }
        info!("workspace updated via git fetch");
    } else {
        std::fs::create_dir_all(workspace).context("create workspace dir")?;
        let output = Command::new("git")
            .args(["clone", &auth_url, "."])
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

/// Inject a PAT into an HTTPS URL for authentication.
/// e.g. `https://github.com/user/repo.git` → `https://x-access-token:{token}@github.com/user/repo.git`
fn inject_token(url: &str, token: Option<&str>) -> String {
    match token {
        Some(t) if !t.is_empty() => {
            if let Some(rest) = url.strip_prefix("https://") {
                format!("https://x-access-token:{t}@{rest}")
            } else {
                url.to_string()
            }
        }
        _ => url.to_string(),
    }
}

/// Create and switch to a feature branch.
/// If the branch already exists (e.g. from a previous failed run), delete and recreate it
/// from the current HEAD so we start clean.
pub async fn create_branch(dir: &Path, name: &str) -> Result<()> {
    // Delete existing local branch if it exists (ignore errors if it doesn't)
    let _ = Command::new("git")
        .args(["branch", "-D", name])
        .current_dir(dir)
        .output()
        .await;

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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_inject_token_https_url() {
        let url = "https://github.com/user/repo.git";
        let result = inject_token(url, Some("mytoken"));
        assert_eq!(result, "https://x-access-token:mytoken@github.com/user/repo.git");
    }

    #[test]
    fn test_inject_token_no_token() {
        let url = "https://github.com/user/repo.git";
        let result = inject_token(url, None);
        assert_eq!(result, url);
    }

    #[test]
    fn test_inject_token_empty_token() {
        let url = "https://github.com/user/repo.git";
        let result = inject_token(url, Some(""));
        assert_eq!(result, url);
    }

    #[test]
    fn test_inject_token_non_https() {
        let url = "git@github.com:user/repo.git";
        let result = inject_token(url, Some("mytoken"));
        assert_eq!(result, url);
    }
}
