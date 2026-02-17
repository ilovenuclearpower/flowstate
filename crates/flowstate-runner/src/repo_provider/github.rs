use std::path::Path;

use async_trait::async_trait;
use tokio::process::Command;
use tracing::info;

use super::{ProviderError, PullRequest, RepoProvider};

pub struct GitHubProvider;

#[async_trait]
impl RepoProvider for GitHubProvider {
    fn name(&self) -> &str {
        "github"
    }

    fn supports_url(&self, repo_url: &str) -> bool {
        repo_url.contains("github.com")
    }

    async fn check_auth(&self, repo_url: &str) -> Result<(), ProviderError> {
        // Check gh auth status
        let auth = Command::new("gh")
            .args(["auth", "status"])
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
        let view = Command::new("gh")
            .args(["repo", "view", repo_url, "--json", "nameWithOwner"])
            .output()
            .await
            .map_err(|e| ProviderError::AuthFailed(format!("gh repo view: {e}")))?;

        if !view.status.success() {
            let stderr = String::from_utf8_lossy(&view.stderr);
            return Err(ProviderError::AuthFailed(format!(
                "cannot access repo {repo_url}: {stderr}"
            )));
        }

        info!("github: authenticated and repo accessible");
        Ok(())
    }

    async fn push_branch(&self, work_dir: &Path, branch: &str) -> Result<(), ProviderError> {
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
        let output = Command::new("gh")
            .args([
                "pr", "create",
                "--title", title,
                "--body", body,
                "--head", branch,
                "--base", base,
                "--json", "number,url",
            ])
            .current_dir(work_dir)
            .output()
            .await
            .map_err(|e| ProviderError::PrFailed(format!("gh pr create: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::PrFailed(format!(
                "gh pr create failed: {stderr}"
            )));
        }

        let stdout = String::from_utf8_lossy(&output.stdout);
        let json: serde_json::Value = serde_json::from_str(&stdout)
            .map_err(|e| ProviderError::PrFailed(format!("parse gh output: {e}")))?;

        let number = json["number"]
            .as_u64()
            .ok_or_else(|| ProviderError::PrFailed("missing number in gh output".into()))?;
        let url = json["url"]
            .as_str()
            .ok_or_else(|| ProviderError::PrFailed("missing url in gh output".into()))?
            .to_string();

        info!("opened PR #{number}: {url}");

        Ok(PullRequest {
            number,
            url,
            branch: branch.to_string(),
        })
    }
}
