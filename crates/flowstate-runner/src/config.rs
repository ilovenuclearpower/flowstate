use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use flowstate_core::claude_run::ClaudeAction;
use flowstate_core::runner::RunnerCapability;

use crate::backend::claude_cli::ClaudeCliBackend;
use crate::backend::gemini_cli::GeminiCliBackend;
use crate::backend::opencode::OpenCodeBackend;
use crate::backend::AgentBackend;

#[derive(Debug, Parser)]
#[command(name = "flowstate-runner", about = "Flowstate build runner")]
pub struct RunnerConfig {
    /// Server URL
    #[arg(long, env = "FLOWSTATE_SERVER_URL", default_value = "http://127.0.0.1:3710")]
    pub server_url: String,

    /// API key for authenticating with the server
    #[arg(long, env = "FLOWSTATE_API_KEY")]
    pub api_key: Option<String>,

    /// Poll interval in seconds
    #[arg(long, default_value = "5")]
    pub poll_interval: u64,

    /// Root directory for workspaces
    #[arg(long, env = "FLOWSTATE_WORKSPACE_ROOT")]
    pub workspace_root: Option<PathBuf>,

    /// Port for the health check endpoint
    #[arg(long, default_value = "3711")]
    pub health_port: u16,

    /// Timeout for research/design/plan/verify actions (seconds).
    /// These are typically shorter than builds.
    #[arg(long, env = "FLOWSTATE_LIGHT_TIMEOUT", default_value = "900")]
    pub light_timeout: u64,

    /// Timeout for build actions (seconds).
    /// Builds can take significantly longer.
    #[arg(long, env = "FLOWSTATE_BUILD_TIMEOUT", default_value = "3600")]
    pub build_timeout: u64,

    /// Grace period after SIGTERM before SIGKILL (seconds).
    #[arg(long, env = "FLOWSTATE_KILL_GRACE", default_value = "10")]
    pub kill_grace_period: u64,

    /// Activity timeout: if no file changes for this many seconds, consider hung (seconds).
    #[arg(long, env = "FLOWSTATE_ACTIVITY_TIMEOUT", default_value = "900")]
    pub activity_timeout: u64,

    /// Maximum number of runs executing simultaneously.
    #[arg(long, env = "FLOWSTATE_MAX_CONCURRENT", default_value = "5")]
    pub max_concurrent: usize,

    /// Maximum number of concurrent Build actions (must be <= max_concurrent).
    #[arg(long, env = "FLOWSTATE_MAX_BUILDS", default_value = "1")]
    pub max_builds: usize,

    /// Seconds to wait for in-progress runs during graceful shutdown before force-killing.
    #[arg(long, env = "FLOWSTATE_SHUTDOWN_TIMEOUT", default_value = "120")]
    pub shutdown_timeout: u64,

    /// Which agentic backend to use: "claude-cli" (default), "gemini-cli", or "opencode"
    #[arg(long, env = "FLOWSTATE_AGENT_BACKEND", default_value = "claude-cli")]
    pub agent_backend: String,

    /// Capability tier this runner advertises: "light", "standard", or "heavy" (default).
    /// A runner at tier X can handle work at tier X and all lower tiers.
    #[arg(long, env = "FLOWSTATE_RUNNER_CAPABILITY", default_value = "heavy")]
    pub runner_capability: String,

    /// For claude-cli backend: override ANTHROPIC_BASE_URL
    /// (enables using vLLM, Ollama, OpenRouter with Anthropic-compatible API)
    #[arg(long, env = "FLOWSTATE_ANTHROPIC_BASE_URL")]
    pub anthropic_base_url: Option<String>,

    /// For claude-cli backend: override ANTHROPIC_AUTH_TOKEN
    #[arg(long, env = "FLOWSTATE_ANTHROPIC_AUTH_TOKEN")]
    pub anthropic_auth_token: Option<String>,

    /// For claude-cli backend: optional model name hint (informational)
    #[arg(long, env = "FLOWSTATE_ANTHROPIC_MODEL")]
    pub anthropic_model: Option<String>,

    /// For opencode backend: provider name (e.g., "openrouter", "ollama")
    #[arg(long, env = "FLOWSTATE_OPENCODE_PROVIDER")]
    pub opencode_provider: Option<String>,

    /// For opencode backend: model identifier
    #[arg(long, env = "FLOWSTATE_OPENCODE_MODEL")]
    pub opencode_model: Option<String>,

    /// For opencode backend: API key
    #[arg(long, env = "FLOWSTATE_OPENCODE_API_KEY")]
    pub opencode_api_key: Option<String>,

    /// For opencode backend: base URL override
    #[arg(long, env = "FLOWSTATE_OPENCODE_BASE_URL")]
    pub opencode_base_url: Option<String>,

    /// For gemini-cli backend: Gemini API key
    #[arg(long, env = "FLOWSTATE_GEMINI_API_KEY")]
    pub gemini_api_key: Option<String>,

    /// For gemini-cli backend: model name (e.g., "gemini-2.5-pro", "gemini-2.5-flash")
    #[arg(long, env = "FLOWSTATE_GEMINI_MODEL")]
    pub gemini_model: Option<String>,

    /// For gemini-cli backend: Google Cloud project ID (for Vertex AI)
    #[arg(long, env = "FLOWSTATE_GEMINI_GCP_PROJECT")]
    pub gemini_gcp_project: Option<String>,

    /// For gemini-cli backend: Google Cloud location (for Vertex AI)
    #[arg(long, env = "FLOWSTATE_GEMINI_GCP_LOCATION")]
    pub gemini_gcp_location: Option<String>,
}

impl RunnerConfig {
    /// Validate configuration constraints. Call after parsing.
    pub fn validate(&self) -> Result<()> {
        if self.max_concurrent < 1 {
            bail!("--max-concurrent must be >= 1, got {}", self.max_concurrent);
        }
        if self.max_builds < 1 {
            bail!("--max-builds must be >= 1, got {}", self.max_builds);
        }
        if self.max_builds > self.max_concurrent {
            bail!(
                "--max-builds ({}) must be <= --max-concurrent ({})",
                self.max_builds,
                self.max_concurrent
            );
        }
        Ok(())
    }

    /// Return the appropriate timeout duration for a given action type.
    pub fn timeout_for_action(&self, action: ClaudeAction) -> Duration {
        let secs = match action {
            ClaudeAction::Build => self.build_timeout,
            _ => self.light_timeout,
        };
        Duration::from_secs(secs)
    }

    /// Returns true if the given action is a Build action (requires the build lock).
    pub fn is_build_action(action: ClaudeAction) -> bool {
        matches!(action, ClaudeAction::Build)
    }

    /// Build the appropriate AgentBackend from configuration.
    pub fn build_backend(&self) -> Result<Box<dyn AgentBackend>> {
        match self.agent_backend.as_str() {
            "claude-cli" => Ok(Box::new(ClaudeCliBackend {
                anthropic_base_url: self.anthropic_base_url.clone(),
                anthropic_auth_token: self.anthropic_auth_token.clone(),
                model: self.anthropic_model.clone(),
            })),
            "gemini-cli" => Ok(Box::new(GeminiCliBackend {
                gemini_api_key: self.gemini_api_key.clone(),
                model: self.gemini_model.clone(),
                google_cloud_project: self.gemini_gcp_project.clone(),
                google_cloud_location: self.gemini_gcp_location.clone(),
            })),
            "opencode" => Ok(Box::new(OpenCodeBackend {
                provider: self
                    .opencode_provider
                    .clone()
                    .unwrap_or_else(|| "ollama".to_string()),
                model: self
                    .opencode_model
                    .clone()
                    .unwrap_or_else(|| "default".to_string()),
                api_key: self.opencode_api_key.clone(),
                api_base_url: self.opencode_base_url.clone(),
            })),
            other => bail!(
                "unknown agent backend: {other}. Supported: claude-cli, gemini-cli, opencode"
            ),
        }
    }

    /// Parse the configured capability tier.
    pub fn capability(&self) -> Result<RunnerCapability> {
        RunnerCapability::parse_str(&self.runner_capability).ok_or_else(|| {
            anyhow::anyhow!(
                "invalid runner capability: {}. Expected: light, standard, heavy",
                self.runner_capability
            )
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> RunnerConfig {
        RunnerConfig {
            server_url: "http://localhost:3710".into(),
            api_key: None,
            poll_interval: 5,
            workspace_root: None,
            health_port: 3711,
            light_timeout: 900,
            build_timeout: 3600,
            kill_grace_period: 10,
            activity_timeout: 900,
            max_concurrent: 5,
            max_builds: 1,
            shutdown_timeout: 120,
            agent_backend: "claude-cli".into(),
            runner_capability: "heavy".into(),
            anthropic_base_url: None,
            anthropic_auth_token: None,
            anthropic_model: None,
            opencode_provider: None,
            opencode_model: None,
            opencode_api_key: None,
            opencode_base_url: None,
            gemini_api_key: None,
            gemini_model: None,
            gemini_gcp_project: None,
            gemini_gcp_location: None,
        }
    }

    #[test]
    fn test_validate_valid_config() {
        assert!(test_config().validate().is_ok());
    }

    #[test]
    fn test_validate_max_concurrent_zero() {
        let mut cfg = test_config();
        cfg.max_concurrent = 0;
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("max-concurrent"),
            "error should mention max-concurrent: {err}"
        );
    }

    #[test]
    fn test_validate_max_builds_zero() {
        let mut cfg = test_config();
        cfg.max_builds = 0;
        let err = cfg.validate().unwrap_err();
        assert!(
            err.to_string().contains("max-builds"),
            "error should mention max-builds: {err}"
        );
    }

    #[test]
    fn test_validate_max_builds_exceeds_concurrent() {
        let mut cfg = test_config();
        cfg.max_builds = 6;
        cfg.max_concurrent = 5;
        let err = cfg.validate().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("6") && msg.contains("5"), "error should mention both values: {msg}");
    }

    #[test]
    fn test_validate_max_builds_equals_concurrent() {
        let mut cfg = test_config();
        cfg.max_builds = 5;
        cfg.max_concurrent = 5;
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn test_timeout_for_action_build_vs_non_build() {
        let cfg = test_config();
        assert_eq!(
            cfg.timeout_for_action(ClaudeAction::Build),
            Duration::from_secs(3600)
        );
        assert_eq!(
            cfg.timeout_for_action(ClaudeAction::Research),
            Duration::from_secs(900)
        );
        assert_eq!(
            cfg.timeout_for_action(ClaudeAction::Verify),
            Duration::from_secs(900)
        );
    }

    #[test]
    fn test_is_build_action() {
        assert!(RunnerConfig::is_build_action(ClaudeAction::Build));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::Research));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::Design));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::Plan));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::Verify));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::ResearchDistill));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::DesignDistill));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::PlanDistill));
        assert!(!RunnerConfig::is_build_action(ClaudeAction::VerifyDistill));
    }

    #[test]
    fn test_capability_heavy() {
        let cfg = test_config();
        let cap = cfg.capability().unwrap();
        assert_eq!(cap, RunnerCapability::Heavy);
    }

    #[test]
    fn test_capability_light() {
        let mut cfg = test_config();
        cfg.runner_capability = "light".into();
        let cap = cfg.capability().unwrap();
        assert_eq!(cap, RunnerCapability::Light);
    }

    #[test]
    fn test_capability_invalid() {
        let mut cfg = test_config();
        cfg.runner_capability = "turbo".into();
        assert!(cfg.capability().is_err());
    }

    #[test]
    fn test_build_backend_claude_cli() {
        let cfg = test_config();
        let backend = cfg.build_backend().unwrap();
        assert_eq!(backend.name(), "claude-cli");
    }

    #[test]
    fn test_build_backend_opencode() {
        let mut cfg = test_config();
        cfg.agent_backend = "opencode".into();
        let backend = cfg.build_backend().unwrap();
        assert_eq!(backend.name(), "opencode");
    }

    #[test]
    fn test_build_backend_gemini_cli() {
        let mut cfg = test_config();
        cfg.agent_backend = "gemini-cli".into();
        let backend = cfg.build_backend().unwrap();
        assert_eq!(backend.name(), "gemini-cli");
        assert_eq!(backend.model_hint(), None);
    }

    #[test]
    fn test_build_backend_gemini_cli_with_model() {
        let mut cfg = test_config();
        cfg.agent_backend = "gemini-cli".into();
        cfg.gemini_model = Some("gemini-2.5-pro".into());
        let backend = cfg.build_backend().unwrap();
        assert_eq!(backend.name(), "gemini-cli");
        assert_eq!(backend.model_hint(), Some("gemini-2.5-pro"));
    }

    #[test]
    fn test_build_backend_unknown() {
        let mut cfg = test_config();
        cfg.agent_backend = "unknown".into();
        assert!(cfg.build_backend().is_err());
    }
}
