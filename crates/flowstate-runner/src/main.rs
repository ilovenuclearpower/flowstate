use std::net::SocketAddr;
use std::sync::Arc;

use anyhow::Result;
use axum::{routing::get, Json, Router};
use clap::Parser;
use flowstate_runner::config::RunnerConfig;
use flowstate_runner::{executor, preflight};
use flowstate_service::{HttpService, TaskService};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{error, info};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = RunnerConfig::parse();
    info!("flowstate-runner starting");
    info!("server: {}", config.server_url);

    let service = Arc::new(match &config.api_key {
        Some(key) => HttpService::with_api_key(&config.server_url, key.clone()),
        None => HttpService::new(&config.server_url),
    });

    // Run preflight checks
    preflight::run_all(&service).await?;

    // Start health endpoint in background
    let health_port = config.health_port;
    tokio::spawn(async move {
        if let Err(e) = run_health_server(health_port).await {
            error!("health server failed: {e}");
        }
    });

    info!("health endpoint: http://127.0.0.1:{health_port}/health");
    info!(
        "entering poll loop (interval: {}s)",
        config.poll_interval
    );

    // Poll loop
    loop {
        match service.claim_claude_run().await {
            Ok(Some(run)) => {
                info!(
                    "claimed run {} (action={}, task={})",
                    run.id, run.action, run.task_id
                );

                let task = match service.get_task(&run.task_id).await {
                    Ok(t) => t,
                    Err(e) => {
                        error!("failed to fetch task for run {}: {e}", run.id);
                        let msg = format!("fetch task: {e}");
                        let _ = service
                            .update_claude_run_status(&run.id, "failed", Some(&msg), None)
                            .await;
                        continue;
                    }
                };

                let project = match service.get_project(&task.project_id).await {
                    Ok(p) => p,
                    Err(e) => {
                        error!("failed to fetch project for run {}: {e}", run.id);
                        let msg = format!("fetch project: {e}");
                        let _ = service
                            .update_claude_run_status(&run.id, "failed", Some(&msg), None)
                            .await;
                        continue;
                    }
                };

                if let Err(e) = executor::dispatch(
                    &service,
                    &run,
                    &task,
                    &project,
                    &config.workspace_root,
                )
                .await
                {
                    error!("run {} failed: {e}", run.id);
                    let msg = format!("{e}");
                    let _ = service
                        .update_claude_run_status(&run.id, "failed", Some(&msg), None)
                        .await;
                }
            }
            Ok(None) => {
                // No work — sleep
            }
            Err(e) => {
                error!("claim failed: {e}");
            }
        }

        tokio::time::sleep(std::time::Duration::from_secs(config.poll_interval)).await;
    }
}

async fn run_health_server(port: u16) -> Result<()> {
    let app = Router::new().route(
        "/health",
        get(|| async { Json(json!({"status": "ok", "role": "runner"})) }),
    );

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
