use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use serde_json::json;
use tokio::process::Command;
use tracing::info;

use super::{AgentBackend, AgentOutput};
use crate::process;

/// OpenCode CLI backend — wraps the `opencode` command-line tool.
///
/// OpenCode is a multi-provider agentic CLI that can read/write files,
/// execute commands, and autonomously work on coding tasks. It supports
/// Anthropic, OpenAI, Google, OpenRouter, and 70+ other providers.
///
/// Configuration is passed via environment variables and
/// `OPENCODE_CONFIG_CONTENT` for headless operation.
pub struct OpenCodeBackend {
    /// Provider name (e.g., "anthropic", "openai", "openrouter")
    pub provider: String,
    /// Model in "provider/model" format (e.g., "anthropic/claude-sonnet-4-5")
    pub model: Option<String>,
    /// API key for the configured provider
    pub api_key: Option<String>,
    /// Optional custom base URL for self-hosted endpoints (vLLM, Ollama, etc.)
    pub base_url: Option<String>,
}

impl OpenCodeBackend {
    /// Map provider name to the environment variable opencode expects.
    fn api_key_env_var(&self) -> &str {
        match self.provider.as_str() {
            "anthropic" => "ANTHROPIC_API_KEY",
            "openai" => "OPENAI_API_KEY",
            "google" | "gemini" => "GOOGLE_API_KEY",
            "openrouter" => "OPENROUTER_API_KEY",
            "groq" => "GROQ_API_KEY",
            _ => "OPENAI_API_KEY",
        }
    }

    /// Build the `OPENCODE_CONFIG_CONTENT` JSON for headless operation.
    ///
    /// Always sets `permission: "allow"` to auto-approve all tool use.
    /// If a custom `base_url` is configured, injects it into the provider options.
    fn config_content(&self) -> String {
        let mut config = json!({
            "permission": "allow",
        });

        if let Some(ref url) = self.base_url {
            config["provider"] = json!({
                &self.provider: {
                    "options": {
                        "baseURL": url
                    }
                }
            });
        }

        config.to_string()
    }

    /// Apply common env vars to a sync command (for preflight).
    fn apply_env(&self, cmd: &mut std::process::Command) {
        cmd.env("OPENCODE_CONFIG_CONTENT", self.config_content());
        if let Some(ref key) = self.api_key {
            cmd.env(self.api_key_env_var(), key);
        }
    }

    /// Apply common env vars to an async command (for run).
    fn apply_env_async(&self, cmd: &mut Command, repo_token: Option<&str>) {
        cmd.env("OPENCODE_CONFIG_CONTENT", self.config_content());
        if let Some(ref key) = self.api_key {
            cmd.env(self.api_key_env_var(), key);
        }
        if let Some(token) = repo_token {
            cmd.env("GITHUB_TOKEN", token);
        }
    }
}

#[async_trait]
impl AgentBackend for OpenCodeBackend {
    fn name(&self) -> &str {
        "opencode"
    }

    fn model_hint(&self) -> Option<&str> {
        self.model.as_deref()
    }

    async fn preflight_check(&self) -> Result<()> {
        // Phase 1: Check opencode binary exists
        let output = std::process::Command::new("opencode")
            .arg("version")
            .output()
            .context("OpenCode CLI is not installed. Install it: https://opencode.ai/docs/")?;
        if !output.status.success() {
            bail!("opencode version failed");
        }
        let version = String::from_utf8_lossy(&output.stdout);
        info!("opencode: {}", version.trim());

        // Phase 2: Check authentication with a small test prompt
        let mut auth_cmd = std::process::Command::new("opencode");
        auth_cmd.args(["run", "Respond with: ok"]);
        if let Some(ref model) = self.model {
            auth_cmd.args(["--model", model]);
        }
        self.apply_env(&mut auth_cmd);

        let output = auth_cmd
            .output()
            .context("failed to run opencode auth check")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "OpenCode CLI not authenticated for provider '{}'. \
                 Set FLOWSTATE_OPENCODE_API_KEY or configure opencode directly.\n\
                 Details: {}",
                self.provider,
                stderr.trim()
            );
        }
        info!("opencode: authenticated (provider={})", self.provider);

        Ok(())
    }

    async fn run(
        &self,
        prompt: &str,
        work_dir: &Path,
        timeout: Duration,
        kill_grace: Duration,
        repo_token: Option<&str>,
        _mcp_env: Option<&super::McpEnv>,
    ) -> Result<AgentOutput> {
        let mut cmd = Command::new("opencode");
        cmd.arg("run").arg(prompt).current_dir(work_dir);

        if let Some(ref model) = self.model {
            cmd.arg("--model").arg(model);
        }

        self.apply_env_async(&mut cmd, repo_token);

        process::run_managed_with_timeout(&mut cmd, work_dir, timeout, kill_grace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_backend_minimal() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "anthropic".into(),
            model: None,
            api_key: None,
            base_url: None,
        }
    }

    fn make_backend_with_model() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "anthropic".into(),
            model: Some("anthropic/claude-sonnet-4-5".into()),
            api_key: Some("test-key".into()),
            base_url: None,
        }
    }

    fn make_backend_openai() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "openai".into(),
            model: Some("openai/gpt-4".into()),
            api_key: Some("test-key".into()),
            base_url: None,
        }
    }

    fn make_backend_custom_url() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "openai".into(),
            model: Some("openai/my-model".into()),
            api_key: Some("test-key".into()),
            base_url: Some("https://vllm.local:8080/v1".into()),
        }
    }

    #[test]
    fn name_returns_opencode() {
        assert_eq!(make_backend_minimal().name(), "opencode");
        assert_eq!(make_backend_with_model().name(), "opencode");
        assert_eq!(make_backend_openai().name(), "opencode");
    }

    #[test]
    fn model_hint_none_when_unset() {
        assert_eq!(make_backend_minimal().model_hint(), None);
    }

    #[test]
    fn model_hint_returns_model() {
        assert_eq!(
            make_backend_with_model().model_hint(),
            Some("anthropic/claude-sonnet-4-5")
        );
        assert_eq!(make_backend_openai().model_hint(), Some("openai/gpt-4"));
    }

    #[test]
    fn api_key_env_var_anthropic() {
        let b = make_backend_with_model();
        assert_eq!(b.api_key_env_var(), "ANTHROPIC_API_KEY");
    }

    #[test]
    fn api_key_env_var_openai() {
        let b = make_backend_openai();
        assert_eq!(b.api_key_env_var(), "OPENAI_API_KEY");
    }

    #[test]
    fn api_key_env_var_google() {
        let b = OpenCodeBackend {
            provider: "google".into(),
            model: None,
            api_key: None,
            base_url: None,
        };
        assert_eq!(b.api_key_env_var(), "GOOGLE_API_KEY");
    }

    #[test]
    fn api_key_env_var_gemini_alias() {
        let b = OpenCodeBackend {
            provider: "gemini".into(),
            model: None,
            api_key: None,
            base_url: None,
        };
        assert_eq!(b.api_key_env_var(), "GOOGLE_API_KEY");
    }

    #[test]
    fn api_key_env_var_openrouter() {
        let b = OpenCodeBackend {
            provider: "openrouter".into(),
            model: None,
            api_key: None,
            base_url: None,
        };
        assert_eq!(b.api_key_env_var(), "OPENROUTER_API_KEY");
    }

    #[test]
    fn api_key_env_var_groq() {
        let b = OpenCodeBackend {
            provider: "groq".into(),
            model: None,
            api_key: None,
            base_url: None,
        };
        assert_eq!(b.api_key_env_var(), "GROQ_API_KEY");
    }

    #[test]
    fn api_key_env_var_unknown_defaults_to_openai() {
        let b = OpenCodeBackend {
            provider: "custom-thing".into(),
            model: None,
            api_key: None,
            base_url: None,
        };
        assert_eq!(b.api_key_env_var(), "OPENAI_API_KEY");
    }

    #[test]
    fn config_content_minimal() {
        let b = make_backend_minimal();
        let content = b.config_content();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["permission"], "allow");
        assert!(parsed.get("provider").is_none());
    }

    #[test]
    fn config_content_with_base_url() {
        let b = make_backend_custom_url();
        let content = b.config_content();
        let parsed: serde_json::Value = serde_json::from_str(&content).unwrap();
        assert_eq!(parsed["permission"], "allow");
        assert_eq!(
            parsed["provider"]["openai"]["options"]["baseURL"],
            "https://vllm.local:8080/v1"
        );
    }

    #[test]
    fn apply_env_sets_config_content() {
        let b = make_backend_minimal();
        let mut cmd = std::process::Command::new("echo");
        b.apply_env(&mut cmd);
        // Verifies no panic; env vars are internal to Command
    }

    #[test]
    fn apply_env_with_api_key() {
        let b = make_backend_with_model();
        let mut cmd = std::process::Command::new("echo");
        b.apply_env(&mut cmd);
    }

    #[test]
    fn apply_env_async_sets_config_content() {
        let b = make_backend_minimal();
        let mut cmd = Command::new("echo");
        b.apply_env_async(&mut cmd, None);
    }

    #[test]
    fn apply_env_async_with_repo_token() {
        let b = make_backend_with_model();
        let mut cmd = Command::new("echo");
        b.apply_env_async(&mut cmd, Some("ghp_test123"));
    }

    #[test]
    fn apply_env_async_with_base_url() {
        let b = make_backend_custom_url();
        let mut cmd = Command::new("echo");
        b.apply_env_async(&mut cmd, Some("ghp_test123"));
    }
}
