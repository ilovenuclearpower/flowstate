use anyhow::{bail, Context, Result};
use flowstate_service::{HttpService, TaskService};
use std::process::Command;
use tracing::info;

use crate::backend::AgentBackend;

/// Run all preflight checks before entering the poll loop.
///
/// Provider-specific preflight checks (e.g. gh CLI for GitHub, token validation
/// for Gitea) are deferred to the first time a project is dispatched — see
/// `pipeline.rs` and `salvage.rs` which call `provider.preflight()` per-run.
pub async fn run_all(service: &HttpService, backend: &dyn AgentBackend) -> Result<()> {
    check_git()?;
    backend.preflight_check().await?;
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn check_git_succeeds() {
        // Git should be available in the nix dev shell
        check_git().unwrap();
    }

    #[tokio::test]
    async fn check_server_health_succeeds() {
        let server = flowstate_server::test_helpers::spawn_test_server().await;
        let svc = HttpService::new(&server.base_url);
        check_server_health(&svc).await.unwrap();
    }

    #[tokio::test]
    async fn check_server_auth_succeeds() {
        let server = flowstate_server::test_helpers::spawn_test_server().await;
        let svc = HttpService::new(&server.base_url);
        check_server_auth(&svc).await.unwrap();
    }
}
