use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use super::{AgentBackend, AgentOutput};

/// A mock backend for testing that writes specified files into the workspace
/// and returns a preconfigured output.
pub struct MockBackend {
    output: AgentOutput,
    /// Files to write into the workspace before returning.
    /// Each entry is (relative_path, content).
    files: Vec<(String, String)>,
}

impl MockBackend {
    /// Create a mock that returns success with the given stdout.
    pub fn success(stdout: &str) -> Self {
        Self {
            output: AgentOutput {
                success: true,
                stdout: stdout.to_string(),
                stderr: String::new(),
                exit_code: 0,
            },
            files: Vec::new(),
        }
    }

    /// Create a mock that returns failure.
    pub fn failure(stderr: &str, exit_code: i32) -> Self {
        Self {
            output: AgentOutput {
                success: false,
                stdout: String::new(),
                stderr: stderr.to_string(),
                exit_code,
            },
            files: Vec::new(),
        }
    }

    /// Add files to write into the workspace before returning.
    pub fn with_files(mut self, files: Vec<(&str, &str)>) -> Self {
        self.files = files
            .into_iter()
            .map(|(p, c)| (p.to_string(), c.to_string()))
            .collect();
        self
    }
}

#[async_trait]
impl AgentBackend for MockBackend {
    fn name(&self) -> &str {
        "mock"
    }

    async fn preflight_check(&self) -> Result<()> {
        Ok(())
    }

    async fn run(
        &self,
        _prompt: &str,
        work_dir: &Path,
        _timeout: Duration,
        _kill_grace: Duration,
        _repo_token: Option<&str>,
        _mcp_env: Option<&super::McpEnv>,
    ) -> Result<AgentOutput> {
        // Write configured files into workspace
        for (path, content) in &self.files {
            let full_path = work_dir.join(path);
            if let Some(parent) = full_path.parent() {
                std::fs::create_dir_all(parent)?;
            }
            std::fs::write(&full_path, content)?;
        }
        Ok(self.output.clone())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn success_creates_ok_output() {
        let mock = MockBackend::success("hello");
        assert!(mock.output.success);
        assert_eq!(mock.output.stdout, "hello");
        assert_eq!(mock.output.stderr, "");
        assert_eq!(mock.output.exit_code, 0);
        assert!(mock.files.is_empty());
    }

    #[test]
    fn failure_creates_err_output() {
        let mock = MockBackend::failure("boom", 42);
        assert!(!mock.output.success);
        assert_eq!(mock.output.stdout, "");
        assert_eq!(mock.output.stderr, "boom");
        assert_eq!(mock.output.exit_code, 42);
    }

    #[test]
    fn with_files_stores_entries() {
        let mock =
            MockBackend::success("ok").with_files(vec![("a.txt", "aaa"), ("sub/b.txt", "bbb")]);
        assert_eq!(mock.files.len(), 2);
        assert_eq!(mock.files[0].0, "a.txt");
        assert_eq!(mock.files[0].1, "aaa");
        assert_eq!(mock.files[1].0, "sub/b.txt");
        assert_eq!(mock.files[1].1, "bbb");
    }

    #[test]
    fn name_is_mock() {
        let mock = MockBackend::success("");
        assert_eq!(mock.name(), "mock");
    }

    #[tokio::test]
    async fn preflight_check_succeeds() {
        let mock = MockBackend::success("");
        mock.preflight_check().await.unwrap();
    }

    #[tokio::test]
    async fn run_returns_output() {
        let mock = MockBackend::success("result");
        let tmp = tempfile::tempdir().unwrap();
        let output = mock
            .run(
                "prompt",
                tmp.path(),
                Duration::from_secs(60),
                Duration::from_secs(5),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(output.success);
        assert_eq!(output.stdout, "result");
    }

    #[tokio::test]
    async fn run_writes_files() {
        let mock = MockBackend::success("ok")
            .with_files(vec![("output.txt", "data"), ("sub/nested.txt", "nested")]);
        let tmp = tempfile::tempdir().unwrap();
        mock.run(
            "prompt",
            tmp.path(),
            Duration::from_secs(60),
            Duration::from_secs(5),
            None,
            None,
        )
        .await
        .unwrap();

        assert_eq!(
            std::fs::read_to_string(tmp.path().join("output.txt")).unwrap(),
            "data"
        );
        assert_eq!(
            std::fs::read_to_string(tmp.path().join("sub/nested.txt")).unwrap(),
            "nested"
        );
    }

    #[tokio::test]
    async fn run_failure_output() {
        let mock = MockBackend::failure("err", 1);
        let tmp = tempfile::tempdir().unwrap();
        let output = mock
            .run(
                "prompt",
                tmp.path(),
                Duration::from_secs(60),
                Duration::from_secs(5),
                None,
                None,
            )
            .await
            .unwrap();
        assert!(!output.success);
        assert_eq!(output.stderr, "err");
        assert_eq!(output.exit_code, 1);
    }

    #[test]
    fn model_hint_default_is_none() {
        let mock = MockBackend::success("");
        assert_eq!(mock.model_hint(), None);
    }
}
