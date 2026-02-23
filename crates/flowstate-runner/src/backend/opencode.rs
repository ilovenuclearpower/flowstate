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

#[cfg(test)]
mod tests {
    use super::*;

    fn make_backend() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "anthropic".into(),
            model: "claude-sonnet-4-5-20250929".into(),
            api_key: Some("test-key".into()),
            api_base_url: Some("https://api.example.com".into()),
        }
    }

    fn make_backend_no_optionals() -> OpenCodeBackend {
        OpenCodeBackend {
            provider: "openai".into(),
            model: "gpt-4".into(),
            api_key: None,
            api_base_url: None,
        }
    }

    #[test]
    fn name_returns_opencode() {
        assert_eq!(make_backend().name(), "opencode");
        assert_eq!(make_backend_no_optionals().name(), "opencode");
    }

    #[test]
    fn model_hint_returns_model() {
        assert_eq!(
            make_backend().model_hint(),
            Some("claude-sonnet-4-5-20250929")
        );
        assert_eq!(make_backend_no_optionals().model_hint(), Some("gpt-4"));
    }

    #[tokio::test]
    async fn preflight_check_bails() {
        let backend = make_backend();
        let err = backend.preflight_check().await.unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }

    #[tokio::test]
    async fn run_bails() {
        let backend = make_backend();
        let tmp = tempfile::tempdir().unwrap();
        let err = backend
            .run(
                "prompt",
                tmp.path(),
                Duration::from_secs(60),
                Duration::from_secs(5),
            )
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not yet implemented"));
    }
}
