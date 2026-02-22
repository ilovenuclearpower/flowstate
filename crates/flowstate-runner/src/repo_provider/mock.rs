use std::path::Path;
use std::sync::atomic::{AtomicU64, Ordering};

use async_trait::async_trait;

use super::{ProviderError, PullRequest, RepoProvider};

/// A mock repo provider for testing that tracks push/PR calls
/// and returns configurable results.
pub struct MockRepoProvider {
    pr_counter: AtomicU64,
    push_fail: bool,
    pr_fail: bool,
}

impl Default for MockRepoProvider {
    fn default() -> Self {
        Self::new()
    }
}

impl MockRepoProvider {
    pub fn new() -> Self {
        Self {
            pr_counter: AtomicU64::new(1),
            push_fail: false,
            pr_fail: false,
        }
    }

    pub fn with_push_fail(mut self) -> Self {
        self.push_fail = true;
        self
    }

    pub fn with_pr_fail(mut self) -> Self {
        self.pr_fail = true;
        self
    }
}

#[async_trait]
impl RepoProvider for MockRepoProvider {
    fn name(&self) -> &str {
        "mock"
    }

    fn supports_url(&self, _repo_url: &str) -> bool {
        true
    }

    async fn check_auth(&self, _repo_url: &str) -> Result<(), ProviderError> {
        Ok(())
    }

    async fn push_branch(&self, _work_dir: &Path, _branch: &str) -> Result<(), ProviderError> {
        if self.push_fail {
            return Err(ProviderError::PushFailed("mock push failure".into()));
        }
        Ok(())
    }

    async fn open_pull_request(
        &self,
        _work_dir: &Path,
        branch: &str,
        _title: &str,
        _body: &str,
        _base: &str,
    ) -> Result<PullRequest, ProviderError> {
        if self.pr_fail {
            return Err(ProviderError::PrFailed("mock PR failure".into()));
        }
        let number = self.pr_counter.fetch_add(1, Ordering::SeqCst);
        Ok(PullRequest {
            number,
            url: format!("https://github.com/test/repo/pull/{number}"),
            branch: branch.to_string(),
        })
    }
}
