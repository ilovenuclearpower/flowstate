use anyhow::{bail, Context, Result};
use flowstate_service::{HttpService, TaskService};
use std::process::Command;
use tracing::info;

/// Run all preflight checks before entering the poll loop.
pub async fn run_all(service: &HttpService) -> Result<()> {
    check_git()?;
    check_claude_cli()?;
    check_claude_auth()?;
    check_gh_cli()?;
    check_gh_auth()?;
    check_server_health(service).await?;
    check_server_auth(service).await?;
    info!("all preflight checks passed");
    Ok(())
}

fn check_git() -> Result<()> {
    let output = Command::new("git")
        .arg("--version")
        .output()
        .context("git is not installed. Install git and try again.")?;
    if !output.status.success() {
        bail!("git --version failed");
    }
    let version = String::from_utf8_lossy(&output.stdout);
    info!("git: {}", version.trim());
    Ok(())
}

fn check_claude_cli() -> Result<()> {
    let output = Command::new("claude")
        .arg("--version")
        .output()
        .context("Claude CLI is not installed. Install it: https://docs.anthropic.com/en/docs/claude-cli")?;
    if !output.status.success() {
        bail!("claude --version failed");
    }
    let version = String::from_utf8_lossy(&output.stdout);
    info!("claude: {}", version.trim());
    Ok(())
}

fn check_claude_auth() -> Result<()> {
    let output = Command::new("claude")
        .args(["-p", "Respond with: ok", "--output-format", "text"])
        .output()
        .context("failed to run claude auth check")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "Claude CLI not authenticated. Run: claude login\nDetails: {}",
            stderr.trim()
        );
    }
    info!("claude: authenticated");
    Ok(())
}

fn check_gh_cli() -> Result<()> {
    let output = Command::new("gh")
        .arg("--version")
        .output()
        .context("GitHub CLI (gh) is not installed. Install it: https://cli.github.com")?;
    if !output.status.success() {
        bail!("gh --version failed");
    }
    let version = String::from_utf8_lossy(&output.stdout);
    info!("gh: {}", version.lines().next().unwrap_or("").trim());
    Ok(())
}

fn check_gh_auth() -> Result<()> {
    let output = Command::new("gh")
        .args(["auth", "status"])
        .output()
        .context("failed to check gh auth status")?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr);
        bail!(
            "GitHub CLI not authenticated. Run: gh auth login\nDetails: {}",
            stderr.trim()
        );
    }
    info!("gh: authenticated");
    Ok(())
}

async fn check_server_health(service: &HttpService) -> Result<()> {
    service
        .health_check()
        .await
        .context("flowstate server is not reachable")?;
    info!("server: reachable");
    Ok(())
}

async fn check_server_auth(service: &HttpService) -> Result<()> {
    service
        .list_projects()
        .await
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("API key authentication failed — check FLOWSTATE_API_KEY")?;
    info!("server: API key valid");
    Ok(())
}
