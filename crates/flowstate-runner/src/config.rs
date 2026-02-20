use std::path::PathBuf;
use std::time::Duration;

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
}

impl RunnerConfig {
    /// Return the appropriate timeout duration for a given action type.
    pub fn timeout_for_action(&self, action: ClaudeAction) -> Duration {
        let secs = match action {
            ClaudeAction::Build => self.build_timeout,
            _ => self.light_timeout,
        };
        Duration::from_secs(secs)
    }
}
