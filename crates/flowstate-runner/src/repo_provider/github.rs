use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::info;

use super::{ProviderError, PullRequest, RepoProvider};

pub struct GitHubProvider {
    /// Optional PAT used to authenticate `gh` CLI calls via GH_TOKEN env var.
    /// When set, this overrides any local `gh auth` session on the host.
    token: Option<String>,
}

impl GitHubProvider {
    pub fn new(token: Option<String>) -> Self {
        Self { token }
    }

    /// Apply GH_TOKEN to a `gh` Command if a token is available.
    fn apply_token(&self, cmd: &mut Command) {
        if let Some(ref token) = self.token {
            cmd.env("GH_TOKEN", token);
        }
    }
}

#[async_trait]
impl RepoProvider for GitHubProvider {
    fn name(&self) -> &str {
        "github"
    }

    fn supports_url(&self, repo_url: &str) -> bool {
        repo_url.contains("github.com")
    }

    async fn check_auth(&self, repo_url: &str) -> Result<(), ProviderError> {
        // Check gh auth status (GH_TOKEN overrides local session if set)
        let mut auth_cmd = Command::new("gh");
        auth_cmd.args(["auth", "status"]);
        self.apply_token(&mut auth_cmd);
        let auth = auth_cmd
            .output()
            .await
            .map_err(|e| ProviderError::AuthFailed(format!("gh auth status: {e}")))?;

        if !auth.status.success() {
            let stderr = String::from_utf8_lossy(&auth.stderr);
            return Err(ProviderError::AuthFailed(format!(
                "gh not authenticated: {stderr}"
            )));
        }

        // Check repo access
        let mut view_cmd = Command::new("gh");
        view_cmd.args(["repo", "view", repo_url, "--json", "nameWithOwner"]);
        self.apply_token(&mut view_cmd);
        let view = view_cmd
            .output()
            .await
            .map_err(|e| ProviderError::AuthFailed(format!("gh repo view: {e}")))?;

        if !view.status.success() {
            let stderr = String::from_utf8_lossy(&view.stderr);
            return Err(ProviderError::AuthFailed(format!(
                "cannot access repo {repo_url}: {stderr}"
            )));
        }

        info!(
            "github: authenticated and repo accessible (token={})",
            if self.token.is_some() { "PAT" } else { "session" }
        );
        Ok(())
    }

    async fn push_branch(&self, work_dir: &Path, branch: &str) -> Result<(), ProviderError> {
        // git push uses the token already injected into the remote URL
        // by workspace::ensure_repo / inject_token
        let output = Command::new("git")
            .args(["push", "-u", "origin", branch])
            .current_dir(work_dir)
            .output()
            .await
            .map_err(|e| ProviderError::PushFailed(format!("git push: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::PushFailed(format!(
                "git push failed: {stderr}"
            )));
        }

        info!("pushed branch {branch} to origin");
        Ok(())
    }

    async fn open_pull_request(
        &self,
        work_dir: &Path,
        branch: &str,
        title: &str,
        body: &str,
        base: &str,
    ) -> Result<PullRequest, ProviderError> {
        let mut cmd = Command::new("gh");
        cmd.args([
            "pr", "create",
            "--title", title,
            "--body", body,
            "--head", branch,
            "--base", base,
        ]);
        cmd.current_dir(work_dir);
        self.apply_token(&mut cmd);

        let output = cmd
            .output()
            .await
            .map_err(|e| ProviderError::PrFailed(format!("gh pr create: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::PrFailed(format!(
                "gh pr create failed: {stderr}"
            )));
        }

        // gh pr create prints the PR URL to stdout, e.g.:
        // https://github.com/owner/repo/pull/42
        let url = String::from_utf8_lossy(&output.stdout).trim().to_string();
        let number = url
            .rsplit('/')
            .next()
            .and_then(|s| s.parse::<u64>().ok())
            .ok_or_else(|| {
                ProviderError::PrFailed(format!("could not parse PR number from: {url}"))
            })?;

        info!("opened PR #{number}: {url}");

        Ok(PullRequest {
            number,
            url,
            branch: branch.to_string(),
        })
    }
}
