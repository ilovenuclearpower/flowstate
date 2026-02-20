use std::path::PathBuf;
use std::time::Duration;

use anyhow::{bail, Result};
use clap::Parser;
use flowstate_core::claude_run::ClaudeAction;

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
}
