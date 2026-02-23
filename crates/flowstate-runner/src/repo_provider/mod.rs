pub mod gitea;
pub mod github;
pub mod mock;

use std::path::Path;

use async_trait::async_trait;
use flowstate_core::project::ProviderType;
use serde::{Deserialize, Serialize};
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

    #[error("not supported by this provider: {0}")]
    NotSupported(String),

    #[error("{0}")]
    Other(String),
}

#[derive(Debug, Clone)]
pub struct PullRequest {
    pub number: u64,
    pub url: String,
    pub branch: String,
}

/// A comment on a pull request (either top-level issue comment or inline review comment).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrComment {
    pub id: u64,
    pub body: String,
    pub author: String,
    pub path: Option<String>,
    pub line: Option<u64>,
    pub created_at: String,
    pub updated_at: Option<String>,
}

/// A review on a pull request.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrReview {
    pub id: u64,
    pub author: String,
    pub state: ReviewState,
    pub body: String,
    pub comments: Vec<PrComment>,
    pub submitted_at: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ReviewState {
    Pending,
    Approved,
    ChangesRequested,
    Commented,
    Dismissed,
}

#[async_trait]
pub trait RepoProvider: Send + Sync {
    fn name(&self) -> &str;

    fn supports_url(&self, repo_url: &str) -> bool;

    /// Run provider-specific preflight checks.
    /// Returns Ok(()) if the provider's external dependencies are available.
    async fn preflight(&self) -> Result<(), ProviderError>;

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

    /// Get the diff for a pull request as a unified diff string.
    async fn get_pr_diff(
        &self,
        _repo_url: &str,
        _pr_number: u64,
    ) -> Result<String, ProviderError> {
        Err(ProviderError::NotSupported(format!(
            "{} does not support get_pr_diff",
            self.name()
        )))
    }

    /// List all comments (issue-level + inline) on a pull request.
    async fn list_pr_comments(
        &self,
        _repo_url: &str,
        _pr_number: u64,
    ) -> Result<Vec<PrComment>, ProviderError> {
        Err(ProviderError::NotSupported(format!(
            "{} does not support list_pr_comments",
            self.name()
        )))
    }

    /// Create a top-level comment on a pull request.
    async fn create_pr_comment(
        &self,
        _repo_url: &str,
        _pr_number: u64,
        _body: &str,
    ) -> Result<PrComment, ProviderError> {
        Err(ProviderError::NotSupported(format!(
            "{} does not support create_pr_comment",
            self.name()
        )))
    }

    /// List reviews on a pull request.
    async fn list_pr_reviews(
        &self,
        _repo_url: &str,
        _pr_number: u64,
    ) -> Result<Vec<PrReview>, ProviderError> {
        Err(ProviderError::NotSupported(format!(
            "{} does not support list_pr_reviews",
            self.name()
        )))
    }

    /// Create a review on a pull request.
    async fn create_pr_review(
        &self,
        _repo_url: &str,
        _pr_number: u64,
        _body: &str,
        _event: ReviewState,
    ) -> Result<PrReview, ProviderError> {
        Err(ProviderError::NotSupported(format!(
            "{} does not support create_pr_review",
            self.name()
        )))
    }
}

/// Build a provider for the given project configuration.
///
/// Resolution order:
/// 1. If `provider_type` is explicitly set, use that provider.
/// 2. If `repo_url` contains "github.com", use GitHub.
/// 3. Otherwise, return Unsupported.
pub fn provider_for_url(
    repo_url: &str,
    token: Option<String>,
    provider_type: Option<ProviderType>,
    skip_tls_verify: bool,
) -> Result<Box<dyn RepoProvider>, ProviderError> {
    // Enforce HTTPS
    if !repo_url.starts_with("https://") {
        return Err(ProviderError::Other(
            "only HTTPS repository URLs are supported".into(),
        ));
    }

    match provider_type {
        Some(ProviderType::Github) => Ok(Box::new(github::GitHubProvider::new(token))),
        Some(ProviderType::Gitea) => {
            Ok(Box::new(gitea::GiteaProvider::new(repo_url, token, skip_tls_verify)?))
        }
        None => {
            // Auto-detect from URL
            let gh = github::GitHubProvider::new(token);
            if gh.supports_url(repo_url) {
                return Ok(Box::new(gh));
            }
            Err(ProviderError::Unsupported(format!(
                "cannot auto-detect provider for {repo_url}; set provider_type on the project",
            )))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn provider_for_github_url() {
        let provider =
            provider_for_url("https://github.com/user/repo", None, None, false)
                .expect("github URL should be supported");
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn provider_for_unsupported_url() {
        let result = provider_for_url("https://gitlab.com/user/repo", None, None, false);
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
        let provider = provider_for_url(
            "https://github.com/user/repo",
            Some("ghp_test123".into()),
            None,
            false,
        )
        .expect("github URL with token should be supported");
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn factory_auto_detects_github() {
        let provider = provider_for_url(
            "https://github.com/user/repo",
            Some("ghp_test".into()),
            None,
            false,
        )
        .unwrap();
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn factory_explicit_gitea() {
        let provider = provider_for_url(
            "https://gitea.example.com/user/repo",
            Some("token123".into()),
            Some(ProviderType::Gitea),
            false,
        )
        .unwrap();
        assert_eq!(provider.name(), "gitea");
    }

    #[test]
    fn factory_rejects_http() {
        let result = provider_for_url(
            "http://insecure.example.com/user/repo",
            None,
            None,
            false,
        );
        assert!(result.is_err());
    }

    #[test]
    fn factory_unknown_url_no_provider_type() {
        let result = provider_for_url(
            "https://unknown.example.com/user/repo",
            Some("token".into()),
            None,
            false,
        );
        let err = result.err().expect("should be an error");
        assert!(err.to_string().contains("set provider_type"));
    }

    #[test]
    fn factory_explicit_github_overrides_url() {
        let provider = provider_for_url(
            "https://custom-gh.example.com/user/repo",
            Some("ghp_test".into()),
            Some(ProviderType::Github),
            false,
        )
        .unwrap();
        assert_eq!(provider.name(), "github");
    }

    #[test]
    fn provider_type_serde_roundtrip() {
        let github = ProviderType::Github;
        let json = serde_json::to_string(&github).unwrap();
        assert_eq!(json, "\"github\"");
        let parsed: ProviderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ProviderType::Github);

        let gitea = ProviderType::Gitea;
        let json = serde_json::to_string(&gitea).unwrap();
        assert_eq!(json, "\"gitea\"");
        let parsed: ProviderType = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed, ProviderType::Gitea);
    }

    #[test]
    fn provider_type_default_is_github() {
        assert_eq!(ProviderType::default(), ProviderType::Github);
    }
}
