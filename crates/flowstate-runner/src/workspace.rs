use anyhow::{bail, Context, Result};
use std::path::Path;
use tokio::process::Command;
use tracing::info;

/// Ensure the workspace directory exists with a git clone/pull.
pub async fn ensure_repo(
    workspace: &Path,
    repo_url: &str,
    repo_token: Option<&str>,
    skip_tls_verify: bool,
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
        let mut fetch_cmd = Command::new("git");
        fetch_cmd.args(["fetch", "origin"]).current_dir(workspace);
        if skip_tls_verify {
            fetch_cmd.env("GIT_SSL_NO_VERIFY", "true");
        }
        let output = fetch_cmd.output().await.context("git fetch")?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!("git fetch failed: {stderr}");
        }
        info!("workspace updated via git fetch");
    } else {
        std::fs::create_dir_all(workspace).context("create workspace dir")?;
        let mut clone_cmd = Command::new("git");
        clone_cmd
            .args(["clone", &auth_url, "."])
            .current_dir(workspace);
        if skip_tls_verify {
            clone_cmd.env("GIT_SSL_NO_VERIFY", "true");
        }
        let output = clone_cmd.output().await.context("git clone")?;

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
        assert_eq!(
            result,
            "https://x-access-token:mytoken@github.com/user/repo.git"
        );
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

    async fn init_test_repo() -> (tempfile::TempDir, std::path::PathBuf) {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();
        tokio::process::Command::new("git")
            .args(["init"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        tokio::process::Command::new("git")
            .args(["config", "user.email", "test@test.com"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        tokio::process::Command::new("git")
            .args(["config", "user.name", "Test"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        tokio::fs::write(dir.join("README.md"), "init")
            .await
            .unwrap();
        tokio::process::Command::new("git")
            .args(["add", "."])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        tokio::process::Command::new("git")
            .args(["commit", "-m", "initial"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        (tmp, dir)
    }

    #[tokio::test]
    async fn test_create_branch() {
        let (_tmp, dir) = init_test_repo().await;
        create_branch(&dir, "feature/test").await.unwrap();

        // Verify we are now on the new branch
        let output = tokio::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(branch, "feature/test");
    }

    #[tokio::test]
    async fn test_create_branch_recreate() {
        let (_tmp, dir) = init_test_repo().await;

        // Create branch the first time
        create_branch(&dir, "feature/dup").await.unwrap();

        // Switch back to the initial branch so we can recreate the feature branch
        tokio::process::Command::new("git")
            .args(["checkout", "-"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();

        // Create the same branch again — should succeed because the function
        // deletes the existing branch first.
        create_branch(&dir, "feature/dup").await.unwrap();

        let output = tokio::process::Command::new("git")
            .args(["branch", "--show-current"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        let branch = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert_eq!(branch, "feature/dup");
    }

    #[tokio::test]
    async fn test_add_and_commit() {
        let (_tmp, dir) = init_test_repo().await;

        // Modify a file so there is something to commit
        tokio::fs::write(dir.join("README.md"), "modified")
            .await
            .unwrap();

        add_and_commit(&dir, "test commit").await.unwrap();

        // Verify the commit was made
        let output = tokio::process::Command::new("git")
            .args(["log", "--oneline", "-1"])
            .current_dir(&dir)
            .output()
            .await
            .unwrap();
        let log = String::from_utf8_lossy(&output.stdout).trim().to_string();
        assert!(
            log.contains("test commit"),
            "expected 'test commit' in log: {log}"
        );
    }

    #[tokio::test]
    async fn test_add_and_commit_no_changes() {
        let (_tmp, dir) = init_test_repo().await;

        // No modifications — working tree is clean
        let result = add_and_commit(&dir, "empty").await;
        assert!(
            result.is_ok(),
            "add_and_commit with no changes should succeed"
        );
    }

    #[tokio::test]
    async fn test_detect_default_branch_no_remote() {
        let (_tmp, dir) = init_test_repo().await;

        // No remote configured, so both symbolic-ref and rev-parse will fail.
        // The function should fall back to "master".
        let branch = detect_default_branch(&dir).await.unwrap();
        assert_eq!(branch, "master");
    }

    #[tokio::test]
    async fn test_ensure_repo_empty_url() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().to_path_buf();

        let result = ensure_repo(&dir, "", None, false).await;
        assert!(result.is_err());
        let err_msg = result.unwrap_err().to_string();
        assert!(
            err_msg.contains("no repo_url configured"),
            "expected 'no repo_url configured' in error: {err_msg}"
        );
    }
}
