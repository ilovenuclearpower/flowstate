pub mod github;

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
pub fn provider_for_url(repo_url: &str) -> Result<Box<dyn RepoProvider>, ProviderError> {
    let gh = github::GitHubProvider;
    if gh.supports_url(repo_url) {
        return Ok(Box::new(gh));
    }

    Err(ProviderError::Unsupported(repo_url.to_string()))
}
