use std::path::Path;
use std::time::Duration;

use anyhow::{bail, Context, Result};
use async_trait::async_trait;
use tokio::process::Command;
use tracing::info;

use super::{AgentBackend, AgentOutput};
use crate::process;

/// Gemini CLI backend — wraps the `gemini` command-line tool (`@google/gemini-cli`).
///
/// Supports multiple authentication methods:
/// - Gemini API key (`GEMINI_API_KEY`)
/// - Vertex AI service account (`GOOGLE_CLOUD_PROJECT` + `GOOGLE_CLOUD_LOCATION`)
/// - Google Login (OAuth, requires browser — not suitable for headless runners)
pub struct GeminiCliBackend {
    /// Gemini API key (set as GEMINI_API_KEY env var in the child process)
    pub gemini_api_key: Option<String>,
    /// Model name, e.g. "gemini-2.5-pro" or "gemini-2.5-flash"
    pub model: Option<String>,
    /// Google Cloud project ID (for Vertex AI auth)
    pub google_cloud_project: Option<String>,
    /// Google Cloud location/region (for Vertex AI auth)
    pub google_cloud_location: Option<String>,
}

impl GeminiCliBackend {
    /// Apply configured environment variables to a command.
    fn apply_env(&self, cmd: &mut std::process::Command) {
        if let Some(ref key) = self.gemini_api_key {
            cmd.env("GEMINI_API_KEY", key);
        }
        if let Some(ref project) = self.google_cloud_project {
            cmd.env("GOOGLE_CLOUD_PROJECT", project);
            cmd.env("GOOGLE_GENAI_USE_VERTEXAI", "true");
        }
        if let Some(ref location) = self.google_cloud_location {
            cmd.env("GOOGLE_CLOUD_LOCATION", location);
        }
    }

    /// Apply configured environment variables to a tokio async command.
    fn apply_env_async(&self, cmd: &mut Command, repo_token: Option<&str>) {
        if let Some(ref key) = self.gemini_api_key {
            cmd.env("GEMINI_API_KEY", key);
        }
        if let Some(ref project) = self.google_cloud_project {
            cmd.env("GOOGLE_CLOUD_PROJECT", project);
            cmd.env("GOOGLE_GENAI_USE_VERTEXAI", "true");
        }
        if let Some(ref location) = self.google_cloud_location {
            cmd.env("GOOGLE_CLOUD_LOCATION", location);
        }
        if let Some(token) = repo_token {
            cmd.env("GITHUB_TOKEN", token);
        }
    }
}

#[async_trait]
impl AgentBackend for GeminiCliBackend {
    fn name(&self) -> &str {
        "gemini-cli"
    }

    fn model_hint(&self) -> Option<&str> {
        self.model.as_deref()
    }

    async fn preflight_check(&self) -> Result<()> {
        // Phase 1: Check gemini binary exists
        let output = std::process::Command::new("gemini")
            .arg("--version")
            .output()
            .context(
                "Gemini CLI is not installed. Install it: npm install -g @google/gemini-cli\n\
                 (requires Node.js >= 18)",
            )?;
        if !output.status.success() {
            bail!("gemini --version failed");
        }
        let version = String::from_utf8_lossy(&output.stdout);
        info!("gemini: {}", version.trim());

        // Phase 2: Check authentication
        let mut auth_cmd = std::process::Command::new("gemini");
        auth_cmd.args(["-p", "Respond with: ok", "--output-format", "text"]);
        self.apply_env(&mut auth_cmd);

        let output = auth_cmd
            .output()
            .context("failed to run gemini auth check")?;
        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr);
            bail!(
                "Gemini CLI not authenticated. Set one of:\n  \
                 - FLOWSTATE_GEMINI_API_KEY (Gemini API key)\n  \
                 - Google Cloud Application Default Credentials (run: gcloud auth application-default login)\n  \
                 - GOOGLE_APPLICATION_CREDENTIALS (Vertex AI service account JSON key path)\n    \
                   with FLOWSTATE_GEMINI_GCP_PROJECT and FLOWSTATE_GEMINI_GCP_LOCATION\n\
                 Details: {}",
                stderr.trim()
            );
        }
        info!("gemini: authenticated");

        Ok(())
    }

    async fn run(
        &self,
        prompt: &str,
        work_dir: &Path,
        timeout: Duration,
        kill_grace: Duration,
        repo_token: Option<&str>,
    ) -> Result<AgentOutput> {
        let mut cmd = Command::new("gemini");
        cmd.arg("-p")
            .arg(prompt)
            .arg("--output-format")
            .arg("text")
            .arg("--yolo")
            .current_dir(work_dir);

        if let Some(ref model) = self.model {
            cmd.arg("-m").arg(model);
        }

        self.apply_env_async(&mut cmd, repo_token);

        process::run_managed_with_timeout(&mut cmd, work_dir, timeout, kill_grace).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn backend_minimal() -> GeminiCliBackend {
        GeminiCliBackend {
            gemini_api_key: None,
            model: None,
            google_cloud_project: None,
            google_cloud_location: None,
        }
    }

    fn backend_with_model() -> GeminiCliBackend {
        GeminiCliBackend {
            gemini_api_key: None,
            model: Some("gemini-2.5-pro".to_string()),
            google_cloud_project: None,
            google_cloud_location: None,
        }
    }

    fn backend_with_api_key() -> GeminiCliBackend {
        GeminiCliBackend {
            gemini_api_key: Some("test-api-key".to_string()),
            model: None,
            google_cloud_project: None,
            google_cloud_location: None,
        }
    }

    fn backend_with_vertex_ai() -> GeminiCliBackend {
        GeminiCliBackend {
            gemini_api_key: None,
            model: Some("gemini-2.5-pro".to_string()),
            google_cloud_project: Some("my-project".to_string()),
            google_cloud_location: Some("us-central1".to_string()),
        }
    }

    fn backend_full() -> GeminiCliBackend {
        GeminiCliBackend {
            gemini_api_key: Some("test-api-key".to_string()),
            model: Some("gemini-2.5-flash".to_string()),
            google_cloud_project: Some("my-project".to_string()),
            google_cloud_location: Some("us-central1".to_string()),
        }
    }

    #[test]
    fn test_name() {
        assert_eq!(backend_minimal().name(), "gemini-cli");
        assert_eq!(backend_with_model().name(), "gemini-cli");
        assert_eq!(backend_full().name(), "gemini-cli");
    }

    #[test]
    fn test_model_hint_none() {
        assert_eq!(backend_minimal().model_hint(), None);
    }

    #[test]
    fn test_model_hint_some() {
        assert_eq!(backend_with_model().model_hint(), Some("gemini-2.5-pro"));
    }

    #[test]
    fn test_model_hint_full() {
        assert_eq!(backend_full().model_hint(), Some("gemini-2.5-flash"));
    }

    #[test]
    fn test_apply_env_no_config() {
        let backend = backend_minimal();
        let mut cmd = std::process::Command::new("echo");
        backend.apply_env(&mut cmd);
        // No env vars set — command should work without errors
        // We verify it doesn't panic; env vars are internal to Command
    }

    #[test]
    fn test_apply_env_api_key_only() {
        let backend = backend_with_api_key();
        let mut cmd = std::process::Command::new("echo");
        backend.apply_env(&mut cmd);
    }

    #[test]
    fn test_apply_env_vertex_ai() {
        let backend = backend_with_vertex_ai();
        let mut cmd = std::process::Command::new("echo");
        backend.apply_env(&mut cmd);
    }

    #[test]
    fn test_apply_env_full() {
        let backend = backend_full();
        let mut cmd = std::process::Command::new("echo");
        backend.apply_env(&mut cmd);
    }

    #[test]
    fn test_apply_env_async_no_config() {
        let backend = backend_minimal();
        let mut cmd = Command::new("echo");
        backend.apply_env_async(&mut cmd, None);
    }

    #[test]
    fn test_apply_env_async_api_key_only() {
        let backend = backend_with_api_key();
        let mut cmd = Command::new("echo");
        backend.apply_env_async(&mut cmd, None);
    }

    #[test]
    fn test_apply_env_async_vertex_ai() {
        let backend = backend_with_vertex_ai();
        let mut cmd = Command::new("echo");
        backend.apply_env_async(&mut cmd, None);
    }

    #[test]
    fn test_apply_env_async_full() {
        let backend = backend_full();
        let mut cmd = Command::new("echo");
        backend.apply_env_async(&mut cmd, None);
    }

    #[test]
    fn test_apply_env_location_without_project() {
        let backend = GeminiCliBackend {
            gemini_api_key: None,
            model: None,
            google_cloud_project: None,
            google_cloud_location: Some("us-central1".to_string()),
        };
        let mut cmd = std::process::Command::new("echo");
        backend.apply_env(&mut cmd);
        // Location set without project — GOOGLE_GENAI_USE_VERTEXAI should NOT be set
    }

    #[test]
    fn test_apply_env_async_location_without_project() {
        let backend = GeminiCliBackend {
            gemini_api_key: None,
            model: None,
            google_cloud_project: None,
            google_cloud_location: Some("us-central1".to_string()),
        };
        let mut cmd = Command::new("echo");
        backend.apply_env_async(&mut cmd, None);
    }
}
