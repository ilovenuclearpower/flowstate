use std::path::PathBuf;

use clap::Parser;

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
}
