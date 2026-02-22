use std::path::Path;
use std::time::Duration;

use anyhow::Result;
use async_trait::async_trait;

use super::{AgentBackend, AgentOutput};

/// OpenCode backend stub for future implementation.
pub struct OpenCodeBackend {
    pub provider: String,
    pub model: String,
    pub api_key: Option<String>,
    pub api_base_url: Option<String>,
}

#[async_trait]
impl AgentBackend for OpenCodeBackend {
    fn name(&self) -> &str {
        "opencode"
    }

    fn model_hint(&self) -> Option<&str> {
        Some(&self.model)
    }

    async fn preflight_check(&self) -> Result<()> {
        anyhow::bail!(
            "OpenCode backend is not yet implemented. \
             Configure FLOWSTATE_AGENT_BACKEND=claude-cli to use Claude CLI."
        )
    }

    async fn run(
        &self,
        _prompt: &str,
        _work_dir: &Path,
        _timeout: Duration,
        _kill_grace: Duration,
    ) -> Result<AgentOutput> {
        anyhow::bail!("OpenCode backend is not yet implemented")
    }
}
