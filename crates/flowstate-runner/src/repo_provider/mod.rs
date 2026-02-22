pub mod github;
pub mod mock;

use std::path::Path;

use async_trait::async_trait;
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ProviderError {
    #[error("auth check failed: {0}")]
    AuthFailed(String),

    #[error("push failed: {0}")]
    PushFailed(String),

    #[error("pr creation failed: {0}")]
    PrFailed(String),

    #[error("unsupported repo URL: {0}")]
    Unsupported(String),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub branch: String,
}

#[async_trait]
pub trait RepoProvider: Send + Sync {
    fn name(&self) -> &str;

    fn supports_url(&self, repo_url: &str) -> bool;

    async fn check_auth(&self, repo_url: &str) -> Result<(), ProviderError>;

    async fn push_branch(&self, work_dir: &Path, branch: &str) -> Result<(), ProviderError>;

    async fn open_pull_request(
        &self,
        work_dir: &Path,
        branch: &str,
        title: &str,
        body: &str,
        base: &str,
    ) -> Result<PullRequest, ProviderError>;
}

/// Return the appropriate provider for a given repo URL.
/// The optional `token` is a PAT used to authenticate with the provider's CLI
/// (e.g. set as `GH_TOKEN` for the GitHub CLI).
pub fn provider_for_url(
    repo_url: &str,
    token: Option<String>,
) -> Result<Box<dyn RepoProvider>, ProviderError> {
    let gh = github::GitHubProvider::new(token);
    if gh.supports_url(repo_url) {
        return Ok(Box::new(gh));
    }

    Err(ProviderError::Unsupported(repo_url.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_for_github_url() {
        let provider = provider_for_url("https://github.com/user/repo", None)
            .expect("github URL should be supported");
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn provider_for_github_ssh_url() {
        // SSH-style URL still contains "github.com"
        let provider = provider_for_url("git@github.com:user/repo.git", None)
            .expect("github SSH URL should be supported");
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn provider_for_unsupported_url() {
        let result = provider_for_url("https://gitlab.com/user/repo", None);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(
            matches!(err, ProviderError::Unsupported(_)),
            "expected Unsupported, got {err:?}"
        );
        assert!(err.to_string().contains("gitlab.com"));
    }

    #[test]
    fn provider_for_github_with_token() {
        let provider =
            provider_for_url("https://github.com/user/repo", Some("ghp_test123".into()))
                .expect("github URL with token should be supported");
        assert_eq!(provider.name(), "github");
    }
}
