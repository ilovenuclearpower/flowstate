pub mod claude_cli;
pub mod gemini_cli;
pub mod mock;
pub mod opencode;

use std::path::{Path, PathBuf};
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

/// Output from an agentic tool run.
#[derive(Debug, Clone)]
pub struct AgentOutput {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// MCP server configuration passed to backends that support it.
#[derive(Debug, Clone)]
pub struct McpEnv {
    /// Path to the flowstate-mcp binary.
    pub mcp_server_path: PathBuf,
    /// Flowstate server URL for the MCP server to connect to.
    pub server_url: String,
    /// Optional API key for the MCP server to use.
    pub api_key: Option<String>,
}

/// Trait for agentic coding tool backends.
///
/// Each backend encapsulates:
/// - How to spawn the agentic tool process
/// - What environment variables / CLI flags to pass
/// - How to validate the tool is available and authenticated
///
/// The trait does NOT handle:
/// - Prompt assembly (handled by flowstate-prompts)
/// - Output file reading (handled by executor.rs)
/// - Process group management (handled by the shared ManagedChild infrastructure)
#[async_trait]
pub trait AgentBackend: Send + Sync {
    /// Human-readable backend name for logging and health reporting.
    fn name(&self) -> &str;

    /// Optional model hint for logging/display purposes.
    fn model_hint(&self) -> Option<&str> {
        None
    }

    /// Whether this backend supports MCP server integration.
    fn supports_mcp(&self) -> bool {
        false
    }

    /// Run preflight checks specific to this backend.
    /// Called once at runner startup.
    async fn preflight_check(&self) -> Result<()>;

    /// Execute an agentic run.
    ///
    /// Given a prompt and workspace directory, spawn the agentic tool
    /// and wait for it to complete (or timeout).
    async fn run(
        &self,
        prompt: &str,
        work_dir: &Path,
        timeout: Duration,
        kill_grace: Duration,
        repo_token: Option<&str>,
        mcp_env: Option<&McpEnv>,
    ) -> Result<AgentOutput>;
}
