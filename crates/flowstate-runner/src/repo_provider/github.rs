use std::path::Path;

use async_trait::async_trait;
use serde::Deserialize;
use tokio::process::Command;
use tracing::info;

use super::{PrComment, PrReview, ProviderError, PullRequest, RepoProvider, ReviewState};

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

    /// Parse "owner/repo" from a GitHub URL.
    fn parse_owner_repo(repo_url: &str) -> Result<(String, String), ProviderError> {
        let url = repo_url.strip_suffix(".git").unwrap_or(repo_url);
        let parts: Vec<&str> = url.trim_end_matches('/').rsplitn(3, '/').collect();
        if parts.len() < 2 {
            return Err(ProviderError::Other("invalid GitHub URL".into()));
        }
        Ok((parts[1].to_string(), parts[0].to_string()))
    }

    /// Run a `gh api` command and return stdout.
    async fn gh_api(&self, args: &[&str]) -> Result<String, ProviderError> {
        let mut cmd = Command::new("gh");
        cmd.arg("api");
        cmd.args(args);
        self.apply_token(&mut cmd);

        let output = cmd
            .output()
            .await
            .map_err(|e| ProviderError::Other(format!("gh api: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Other(format!("gh api failed: {stderr}")));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
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

    async fn preflight(&self) -> Result<(), ProviderError> {
        // Check gh CLI is installed
        let output = std::process::Command::new("gh")
            .arg("--version")
            .output()
            .map_err(|_| {
                ProviderError::Other(
                    "GitHub CLI (gh) is not installed. Install it: https://cli.github.com".into(),
                )
            })?;
        if !output.status.success() {
            return Err(ProviderError::Other("gh --version failed".into()));
        }

        // Check gh auth
        let mut auth_cmd = std::process::Command::new("gh");
        auth_cmd.args(["auth", "status"]);
        if let Some(ref token) = self.token {
            auth_cmd.env("GH_TOKEN", token);
        }
        let auth = auth_cmd.output().map_err(|e| {
            ProviderError::AuthFailed(format!("failed to check gh auth status: {e}"))
        })?;
        if !auth.status.success() {
            let stderr = String::from_utf8_lossy(&auth.stderr);
            return Err(ProviderError::AuthFailed(format!(
                "GitHub CLI not authenticated. Run: gh auth login\nDetails: {}",
                stderr.trim()
            )));
        }

        let version = String::from_utf8_lossy(&output.stdout);
        info!(
            "gh: {} (token={})",
            version.lines().next().unwrap_or("").trim(),
            if self.token.is_some() {
                "PAT"
            } else {
                "session"
            }
        );
        Ok(())
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
            if self.token.is_some() {
                "PAT"
            } else {
                "session"
            }
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
            "pr", "create", "--title", title, "--body", body, "--head", branch, "--base", base,
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

    async fn get_pr_diff(&self, repo_url: &str, pr_number: u64) -> Result<String, ProviderError> {
        let (owner, repo) = Self::parse_owner_repo(repo_url)?;
        let mut cmd = Command::new("gh");
        cmd.args([
            "pr",
            "diff",
            &pr_number.to_string(),
            "--repo",
            &format!("{owner}/{repo}"),
        ]);
        self.apply_token(&mut cmd);

        let output = cmd
            .output()
            .await
            .map_err(|e| ProviderError::Other(format!("gh pr diff: {e}")))?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            return Err(ProviderError::Other(format!("gh pr diff failed: {stderr}")));
        }

        Ok(String::from_utf8_lossy(&output.stdout).to_string())
    }

    async fn list_pr_comments(
        &self,
        repo_url: &str,
        pr_number: u64,
    ) -> Result<Vec<PrComment>, ProviderError> {
        let (owner, repo) = Self::parse_owner_repo(repo_url)?;

        // Issue-level comments
        let issue_json = self
            .gh_api(&[&format!("repos/{owner}/{repo}/issues/{pr_number}/comments")])
            .await?;

        let issue_comments: Vec<GhComment> = serde_json::from_str(&issue_json).unwrap_or_default();

        // Inline review comments
        let review_json = self
            .gh_api(&[&format!("repos/{owner}/{repo}/pulls/{pr_number}/comments")])
            .await?;

        let review_comments: Vec<GhReviewComment> =
            serde_json::from_str(&review_json).unwrap_or_default();

        let mut comments: Vec<PrComment> = issue_comments
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

        comments.extend(review_comments.into_iter().map(|c| PrComment {
            id: c.id,
            body: c.body,
            author: c.user.login,
            path: Some(c.path),
            line: c.line,
            created_at: c.created_at,
            updated_at: Some(c.updated_at),
        }));

        Ok(comments)
    }

    async fn create_pr_comment(
        &self,
        repo_url: &str,
        pr_number: u64,
        body: &str,
    ) -> Result<PrComment, ProviderError> {
        let (owner, repo) = Self::parse_owner_repo(repo_url)?;
        let json = self
            .gh_api(&[
                &format!("repos/{owner}/{repo}/issues/{pr_number}/comments"),
                "-f",
                &format!("body={body}"),
            ])
            .await?;

        let c: GhComment = serde_json::from_str(&json)
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
        repo_url: &str,
        pr_number: u64,
    ) -> Result<Vec<PrReview>, ProviderError> {
        let (owner, repo) = Self::parse_owner_repo(repo_url)?;
        let json = self
            .gh_api(&[&format!("repos/{owner}/{repo}/pulls/{pr_number}/reviews")])
            .await?;

        let gh_reviews: Vec<GhReview> = serde_json::from_str(&json).unwrap_or_default();

        let mut reviews = Vec::new();
        for r in gh_reviews {
            let state = match r.state.as_str() {
                "APPROVED" => ReviewState::Approved,
                "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
                "COMMENTED" => ReviewState::Commented,
                "DISMISSED" => ReviewState::Dismissed,
                _ => ReviewState::Pending,
            };

            // Fetch inline comments for this review
            let comments_json = self
                .gh_api(&[&format!(
                    "repos/{owner}/{repo}/pulls/{pr_number}/reviews/{}/comments",
                    r.id
                )])
                .await
                .unwrap_or_else(|_| "[]".to_string());

            let gh_comments: Vec<GhReviewComment> =
                serde_json::from_str(&comments_json).unwrap_or_default();

            let comments = gh_comments
                .into_iter()
                .map(|c| PrComment {
                    id: c.id,
                    body: c.body,
                    author: c.user.login,
                    path: Some(c.path),
                    line: c.line,
                    created_at: c.created_at,
                    updated_at: Some(c.updated_at),
                })
                .collect();

            reviews.push(PrReview {
                id: r.id,
                author: r.user.login,
                state,
                body: r.body.unwrap_or_default(),
                comments,
                submitted_at: r.submitted_at,
            });
        }

        Ok(reviews)
    }

    async fn create_pr_review(
        &self,
        repo_url: &str,
        pr_number: u64,
        body: &str,
        event: ReviewState,
    ) -> Result<PrReview, ProviderError> {
        let (owner, repo) = Self::parse_owner_repo(repo_url)?;
        let event_str = match event {
            ReviewState::Approved => "APPROVE",
            ReviewState::ChangesRequested => "REQUEST_CHANGES",
            ReviewState::Commented => "COMMENT",
            _ => "COMMENT",
        };

        let json = self
            .gh_api(&[
                &format!("repos/{owner}/{repo}/pulls/{pr_number}/reviews"),
                "-f",
                &format!("body={body}"),
                "-f",
                &format!("event={event_str}"),
            ])
            .await?;

        let r: GhReview = serde_json::from_str(&json)
            .map_err(|e| ProviderError::Other(format!("parse review response: {e}")))?;

        let state = match r.state.as_str() {
            "APPROVED" => ReviewState::Approved,
            "CHANGES_REQUESTED" => ReviewState::ChangesRequested,
            "COMMENTED" => ReviewState::Commented,
            "DISMISSED" => ReviewState::Dismissed,
            _ => ReviewState::Pending,
        };

        Ok(PrReview {
            id: r.id,
            author: r.user.login,
            state,
            body: r.body.unwrap_or_default(),
            comments: Vec::new(),
            submitted_at: r.submitted_at,
        })
    }
}

// GitHub API response structs for deserialization

#[derive(Deserialize)]
struct GhUser {
    login: String,
}

#[derive(Deserialize)]
struct GhComment {
    id: u64,
    body: String,
    user: GhUser,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct GhReviewComment {
    id: u64,
    body: String,
    user: GhUser,
    path: String,
    line: Option<u64>,
    created_at: String,
    updated_at: String,
}

#[derive(Deserialize)]
struct GhReview {
    id: u64,
    user: GhUser,
    state: String,
    body: Option<String>,
    submitted_at: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_owner_repo_https() {
        let (owner, repo) =
            GitHubProvider::parse_owner_repo("https://github.com/user/repo").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn parse_owner_repo_https_with_git_suffix() {
        let (owner, repo) =
            GitHubProvider::parse_owner_repo("https://github.com/user/repo.git").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(repo, "repo");
    }

    #[test]
    fn parse_owner_repo_with_trailing_slash() {
        let (owner, repo) =
            GitHubProvider::parse_owner_repo("https://github.com/user/repo/").unwrap();
        assert_eq!(owner, "user");
        assert_eq!(repo, "repo");
    }
}
