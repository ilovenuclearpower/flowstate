use std::path::Path;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use tokio::process::Command;
use tracing::info;

use super::{PrComment, PrReview, ProviderError, PullRequest, RepoProvider, ReviewState};

#[derive(Debug)]
pub struct GiteaProvider {
    /// Base URL of the Gitea instance (e.g. "https://gitea.example.com").
    base_url: String,
    /// Owner parsed from the repo URL.
    owner: String,
    /// Repo name parsed from the repo URL.
    repo: String,
    /// API token for authentication.
    token: String,
    /// HTTP client (configured for TLS).
    client: reqwest::Client,
    /// Whether to skip TLS verification for git commands.
    skip_tls_verify: bool,
}

impl GiteaProvider {
    pub fn new(
        repo_url: &str,
        token: Option<String>,
        skip_tls_verify: bool,
    ) -> Result<Self, ProviderError> {
        let token = token.ok_or_else(|| {
            ProviderError::AuthFailed("Gitea requires an API token".into())
        })?;

        let (base_url, owner, repo) = parse_gitea_url(repo_url)?;

        let client = reqwest::Client::builder()
            .danger_accept_invalid_certs(skip_tls_verify)
            .user_agent("flowstate-runner")
            .build()
            .map_err(|e| ProviderError::Other(format!("HTTP client init: {e}")))?;

        Ok(Self {
            base_url,
            owner,
            repo,
            token,
            client,
            skip_tls_verify,
        })
    }

    fn api_url(&self, path: &str) -> String {
        format!("{}/api/v1{}", self.base_url, path)
    }

    async fn api_get(&self, path: &str) -> Result<reqwest::Response, ProviderError> {
        self.client
            .get(self.api_url(path))
            .header("Authorization", format!("token {}", self.token))
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("HTTP request failed: {e}")))
    }

    async fn api_post<T: Serialize + Send + Sync>(
        &self,
        path: &str,
        body: &T,
    ) -> Result<reqwest::Response, ProviderError> {
        self.client
            .post(self.api_url(path))
            .header("Authorization", format!("token {}", self.token))
            .json(body)
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("HTTP request failed: {e}")))
    }
}

/// Parse a Gitea repo URL into (base_url, owner, repo).
///
/// Supports:
///   https://gitea.example.com/owner/repo.git
///   https://gitea.example.com/owner/repo
///   https://gitea.example.com:3000/owner/repo
fn parse_gitea_url(url: &str) -> Result<(String, String, String), ProviderError> {
    let url = url.strip_suffix(".git").unwrap_or(url);
    let parsed = url::Url::parse(url)
        .map_err(|e| ProviderError::Other(format!("invalid URL: {e}")))?;

    let segments: Vec<&str> = parsed
        .path_segments()
        .ok_or_else(|| ProviderError::Other("URL has no path".into()))?
        .filter(|s| !s.is_empty())
        .collect();

    if segments.len() < 2 {
        return Err(ProviderError::Other(
            "URL must contain owner/repo path segments".into(),
        ));
    }

    let host = parsed.host_str().ok_or_else(|| {
        ProviderError::Other("URL has no host".into())
    })?;

    let base = if let Some(port) = parsed.port() {
        format!("{}://{}:{}", parsed.scheme(), host, port)
    } else {
        format!("{}://{}", parsed.scheme(), host)
    };

    let owner = segments[0].to_string();
    let repo = segments[1].to_string();
    Ok((base, owner, repo))
}

#[async_trait]
impl RepoProvider for GiteaProvider {
    fn name(&self) -> &str {
        "gitea"
    }

    fn supports_url(&self, _repo_url: &str) -> bool {
        // Gitea URLs have no universal pattern; the factory handles routing via provider_type.
        true
    }

    async fn preflight(&self) -> Result<(), ProviderError> {
        // No external CLI needed for Gitea — we use reqwest directly.
        Ok(())
    }

    async fn check_auth(&self, _repo_url: &str) -> Result<(), ProviderError> {
        // Verify token validity
        let resp = self.api_get("/user").await?;
        if !resp.status().is_success() {
            return Err(ProviderError::AuthFailed(format!(
                "Gitea auth failed (status {})",
                resp.status()
            )));
        }

        // Verify repo access
        let resp = self
            .api_get(&format!("/repos/{}/{}", self.owner, self.repo))
            .await?;
        if !resp.status().is_success() {
            return Err(ProviderError::AuthFailed(format!(
                "cannot access repo {}/{} (status {})",
                self.owner,
                self.repo,
                resp.status()
            )));
        }

        info!(
            "gitea: authenticated and repo {}/{} accessible",
            self.owner, self.repo
        );
        Ok(())
    }

    async fn push_branch(&self, work_dir: &Path, branch: &str) -> Result<(), ProviderError> {
        let mut cmd = Command::new("git");
        cmd.args(["push", "-u", "origin", branch]);
        cmd.current_dir(work_dir);
        if self.skip_tls_verify {
            cmd.env("GIT_SSL_NO_VERIFY", "true");
        }

        let output = cmd
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
        _work_dir: &Path,
        branch: &str,
        title: &str,
        body: &str,
        base: &str,
    ) -> Result<PullRequest, ProviderError> {
        #[derive(Serialize)]
        struct CreatePr<'a> {
            title: &'a str,
            body: &'a str,
            head: &'a str,
            base: &'a str,
        }

        let req_body = CreatePr {
            title,
            body,
            head: branch,
            base,
        };

        let resp = self
            .api_post(
                &format!("/repos/{}/{}/pulls", self.owner, self.repo),
                &req_body,
            )
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::PrFailed(format!(
                "Gitea PR creation failed (status {status}): {text}"
            )));
        }

        let pr: GiteaPr = resp
            .json()
            .await
            .map_err(|e| ProviderError::PrFailed(format!("parse PR response: {e}")))?;

        info!("opened PR #{}: {}", pr.number, pr.html_url);

        Ok(PullRequest {
            number: pr.number,
            url: pr.html_url,
            branch: branch.to_string(),
        })
    }

    async fn get_pr_diff(
        &self,
        _repo_url: &str,
        pr_number: u64,
    ) -> Result<String, ProviderError> {
        let resp = self
            .client
            .get(self.api_url(&format!(
                "/repos/{}/{}/pulls/{pr_number}.diff",
                self.owner, self.repo
            )))
            .header("Authorization", format!("token {}", self.token))
            .header("Accept", "text/plain")
            .send()
            .await
            .map_err(|e| ProviderError::Other(format!("HTTP request failed: {e}")))?;

        if !resp.status().is_success() {
            return Err(ProviderError::Other(format!(
                "get PR diff failed (status {})",
                resp.status()
            )));
        }

        resp.text()
            .await
            .map_err(|e| ProviderError::Other(format!("read diff body: {e}")))
    }

    async fn list_pr_comments(
        &self,
        _repo_url: &str,
        pr_number: u64,
    ) -> Result<Vec<PrComment>, ProviderError> {
        // Issue-level comments (PRs are issues in Gitea)
        let resp = self
            .api_get(&format!(
                "/repos/{}/{}/issues/{pr_number}/comments",
                self.owner, self.repo
            ))
            .await?;

        let issue_comments: Vec<GiteaComment> = if resp.status().is_success() {
            resp.json().await.unwrap_or_default()
        } else {
            Vec::new()
        };

        let comments = issue_comments
            .into_iter()
            .map(|c| PrComment {
                id: c.id,
                body: c.body,
                author: c.user.login,
                path: None,
                line: None,
                created_at: c.created_at,
                updated_at: Some(c.updated_at),
            })
            .collect();

        Ok(comments)
    }

    async fn create_pr_comment(
        &self,
        _repo_url: &str,
        pr_number: u64,
        body: &str,
    ) -> Result<PrComment, ProviderError> {
        #[derive(Serialize)]
        struct CommentBody<'a> {
            body: &'a str,
        }

        let resp = self
            .api_post(
                &format!(
                    "/repos/{}/{}/issues/{pr_number}/comments",
                    self.owner, self.repo
                ),
                &CommentBody { body },
            )
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Other(format!(
                "create comment failed (status {status}): {text}"
            )));
        }

        let c: GiteaComment = resp
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("parse comment response: {e}")))?;

        Ok(PrComment {
            id: c.id,
            body: c.body,
            author: c.user.login,
            path: None,
            line: None,
            created_at: c.created_at,
            updated_at: Some(c.updated_at),
        })
    }

    async fn list_pr_reviews(
        &self,
        _repo_url: &str,
        pr_number: u64,
    ) -> Result<Vec<PrReview>, ProviderError> {
        let resp = self
            .api_get(&format!(
                "/repos/{}/{}/pulls/{pr_number}/reviews",
                self.owner, self.repo
            ))
            .await?;

        let gitea_reviews: Vec<GiteaReview> = if resp.status().is_success() {
            resp.json().await.unwrap_or_default()
        } else {
            Vec::new()
        };

        let reviews = gitea_reviews
            .into_iter()
            .map(|r| {
                let state = match r.state.as_str() {
                    "APPROVED" => ReviewState::Approved,
                    "REQUEST_CHANGES" => ReviewState::ChangesRequested,
                    "COMMENT" => ReviewState::Commented,
                    "DISMISSED" => ReviewState::Dismissed,
                    _ => ReviewState::Pending,
                };

                PrReview {
                    id: r.id,
                    author: r.user.login,
                    state,
                    body: r.body,
                    comments: Vec::new(),
                    submitted_at: r.submitted_at,
                }
            })
            .collect();

        Ok(reviews)
    }

    async fn create_pr_review(
        &self,
        _repo_url: &str,
        pr_number: u64,
        body: &str,
        event: ReviewState,
    ) -> Result<PrReview, ProviderError> {
        let event_str = match event {
            ReviewState::Approved => "APPROVED",
            ReviewState::ChangesRequested => "REQUEST_CHANGES",
            ReviewState::Commented => "COMMENT",
            _ => "COMMENT",
        };

        #[derive(Serialize)]
        struct ReviewBody<'a> {
            body: &'a str,
            event: &'a str,
        }

        let resp = self
            .api_post(
                &format!(
                    "/repos/{}/{}/pulls/{pr_number}/reviews",
                    self.owner, self.repo
                ),
                &ReviewBody { body, event: event_str },
            )
            .await?;

        if !resp.status().is_success() {
            let status = resp.status();
            let text = resp.text().await.unwrap_or_default();
            return Err(ProviderError::Other(format!(
                "create review failed (status {status}): {text}"
            )));
        }

        let r: GiteaReview = resp
            .json()
            .await
            .map_err(|e| ProviderError::Other(format!("parse review response: {e}")))?;

        let state = match r.state.as_str() {
            "APPROVED" => ReviewState::Approved,
            "REQUEST_CHANGES" => ReviewState::ChangesRequested,
            "COMMENT" => ReviewState::Commented,
            "DISMISSED" => ReviewState::Dismissed,
            _ => ReviewState::Pending,
        };

        Ok(PrReview {
            id: r.id,
            author: r.user.login,
            state,
            body: r.body,
            comments: Vec::new(),
            submitted_at: r.submitted_at,
        })
    }
}

// Gitea API response structs

#[derive(Deserialize)]
struct GiteaUser {
    login: String,
}

#[derive(Deserialize)]
struct GiteaPr {
    number: u64,
    html_url: String,
}

#[derive(Deserialize)]
struct GiteaComment {
    id: u64,
    body: String,
    user: GiteaUser,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct GiteaReview {
    id: u64,
    user: GiteaUser,
    state: String,
    body: String,
    submitted_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_gitea_url_standard() {
        let (base, owner, repo) =
            parse_gitea_url("https://gitea.example.com/myorg/myrepo").unwrap();
        assert_eq!(base, "https://gitea.example.com");
        assert_eq!(owner, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_gitea_url_with_dot_git() {
        let (_, _, repo) =
            parse_gitea_url("https://gitea.example.com/myorg/myrepo.git").unwrap();
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_gitea_url_with_port() {
        let (base, owner, repo) =
            parse_gitea_url("https://gitea.example.com:3000/myorg/myrepo").unwrap();
        assert_eq!(base, "https://gitea.example.com:3000");
        assert_eq!(owner, "myorg");
        assert_eq!(repo, "myrepo");
    }

    #[test]
    fn parse_gitea_url_missing_repo() {
        assert!(parse_gitea_url("https://gitea.example.com/onlyone").is_err());
    }

    #[test]
    fn gitea_requires_token() {
        let result = GiteaProvider::new(
            "https://gitea.example.com/user/repo",
            None,
            false,
        );
        assert!(result.is_err());
        let err = result.unwrap_err();
        assert!(matches!(err, ProviderError::AuthFailed(_)));
    }

    #[test]
    fn gitea_construction_with_token() {
        let provider = GiteaProvider::new(
            "https://gitea.example.com/user/repo",
            Some("token123".into()),
            false,
        )
        .unwrap();
        assert_eq!(provider.name(), "gitea");
        assert_eq!(provider.owner, "user");
        assert_eq!(provider.repo, "repo");
        assert_eq!(provider.base_url, "https://gitea.example.com");
    }

    #[test]
    fn gitea_construction_with_port() {
        let provider = GiteaProvider::new(
            "https://gitea.example.com:3000/org/project.git",
            Some("token123".into()),
            true,
        )
        .unwrap();
        assert_eq!(provider.base_url, "https://gitea.example.com:3000");
        assert_eq!(provider.owner, "org");
        assert_eq!(provider.repo, "project");
        assert!(provider.skip_tls_verify);
    }
}
