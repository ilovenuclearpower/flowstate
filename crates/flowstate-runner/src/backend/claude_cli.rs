use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use tracing::info;

use super::{AgentBackend, AgentOutput};
use crate::process;

/// Claude CLI backend — wraps the `claude` command-line tool.
///
/// Supports optional endpoint overrides via `anthropic_base_url` and
/// `anthropic_auth_token`, enabling use with vLLM, Ollama, OpenRouter,
/// or any Anthropic-compatible API.
pub struct ClaudeCliBackend {
    /// Override for ANTHROPIC_BASE_URL (e.g., for vLLM, Ollama, OpenRouter)
    pub anthropic_base_url: Option<String>,
    /// Override for ANTHROPIC_AUTH_TOKEN
    pub anthropic_auth_token: Option<String>,
    /// Model name hint for logging
    pub model: Option<String>,
}

#[async_trait]
impl AgentBackend for ClaudeCliBackend {
    fn name(&self) -> &str {
        if self.anthropic_base_url.is_some() {
            "claude-cli/custom"
        } else {
            "claude-cli"
        }
    }

    fn model_hint(&self) -> Option<&str> {
        self.model.as_deref()
    }

    async fn preflight_check(&self) -> Result<()> {
        // Check claude binary exists
        let output = std::process::Command::new("claude")
            .arg("--version")
            .output()
            .context(
                "Claude CLI is not installed. Install it: https://docs.anthropic.com/en/docs/claude-cli",
            )?;
        if !output.status.success() {
            bail!("claude --version failed");
        }
        let version = String::from_utf8_lossy(&output.stdout);
        info!("claude: {}", version.trim());

        // Check authentication
        let mut auth_cmd = std::process::Command::new("claude");
        auth_cmd.args(["-p", "Respond with: ok", "--output-format", "text"]);

        if let Some(ref url) = self.anthropic_base_url {
            auth_cmd.env("ANTHROPIC_BASE_URL", url);
        }
        if let Some(ref token) = self.anthropic_auth_token {
            auth_cmd.env("ANTHROPIC_AUTH_TOKEN", token);
        }

        let output = auth_cmd
            .output()
            .context("failed to run claude auth check")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            let endpoint_info = match &self.anthropic_base_url {
                Some(url) => format!(" (endpoint: {url})"),
                None => String::new(),
            };
            bail!(
                "Claude CLI not authenticated{endpoint_info}. Run: claude login\nDetails: {}",
                stderr.trim()
            );
        }
        info!("claude: authenticated");

        Ok(())
    }

    async fn run(
        &self,
        prompt: &str,
        work_dir: &Path,
        timeout: Duration,
        kill_grace: Duration,
    ) -> Result<AgentOutput> {
        let mut cmd = Command::new("claude");
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("text")
            .arg("--dangerously-skip-permissions")
            .current_dir(work_dir);

        // Apply endpoint overrides
        if let Some(ref url) = self.anthropic_base_url {
            cmd.env("ANTHROPIC_BASE_URL", url);
        }
        if let Some(ref token) = self.anthropic_auth_token {
            cmd.env("ANTHROPIC_AUTH_TOKEN", token);
        }

        process::run_managed_with_timeout(&mut cmd, work_dir, timeout, kill_grace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn name_default() {
        let b = ClaudeCliBackend {
            anthropic_base_url: None,
            anthropic_auth_token: None,
            model: None,
        };
        assert_eq!(b.name(), "claude-cli");
    }

    #[test]
    fn name_custom_endpoint() {
        let b = ClaudeCliBackend {
            anthropic_base_url: Some("https://custom.example.com".into()),
            anthropic_auth_token: None,
            model: None,
        };
        assert_eq!(b.name(), "claude-cli/custom");
    }

    #[test]
    fn model_hint_none() {
        let b = ClaudeCliBackend {
            anthropic_base_url: None,
            anthropic_auth_token: None,
            model: None,
        };
        assert_eq!(b.model_hint(), None);
    }

    #[test]
    fn model_hint_some() {
        let b = ClaudeCliBackend {
            anthropic_base_url: None,
            anthropic_auth_token: None,
            model: Some("claude-opus-4-6".into()),
        };
        assert_eq!(b.model_hint(), Some("claude-opus-4-6"));
    }

    #[test]
    fn name_with_both_overrides() {
        let b = ClaudeCliBackend {
            anthropic_base_url: Some("https://vllm.local".into()),
            anthropic_auth_token: Some("token123".into()),
            model: Some("my-model".into()),
        };
        assert_eq!(b.name(), "claude-cli/custom");
        assert_eq!(b.model_hint(), Some("my-model"));
    }
}
