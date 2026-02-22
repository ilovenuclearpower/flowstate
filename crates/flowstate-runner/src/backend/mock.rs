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
