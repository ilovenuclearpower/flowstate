use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use axum::{routing::get, Json, Router};
use clap::Parser;
use flowstate_core::claude_run::ClaudeAction;
use flowstate_runner::config::RunnerConfig;
use flowstate_runner::{executor, preflight, salvage};
use flowstate_service::{HttpService, TaskService};
use serde_json::json;
use tokio::net::TcpListener;
use tracing::{error, info, warn};

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
    info!(
        "timeouts: light={}s, build={}s, kill_grace={}s",
        config.light_timeout, config.build_timeout, config.kill_grace_period
    );

    // Generate runner ID from HOSTNAME env var or UUID
    let runner_id = std::env::var("HOSTNAME")
        .or_else(|_| std::env::var("HOST"))
        .unwrap_or_else(|_| uuid::Uuid::new_v4().to_string());
    info!("runner id: {runner_id}");

    let mut svc = match &config.api_key {
        Some(key) => HttpService::with_api_key(&config.server_url, key.clone()),
        None => HttpService::new(&config.server_url),
    };
    svc.set_runner_id(runner_id);
    let service = Arc::new(svc);

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

                let timeout = config.timeout_for_action(run.action);

                // Spawn heartbeat task that sends periodic progress while dispatch runs
                let heartbeat_service = service.clone();
                let heartbeat_run_id = run.id.clone();
                let heartbeat = tokio::spawn(async move {
                    heartbeat_loop(&heartbeat_service, &heartbeat_run_id).await;
                });

                let result = tokio::time::timeout(
                    timeout,
                    executor::dispatch(&service, &run, &task, &project, &config),
                )
                .await;

                // Stop heartbeat
                heartbeat.abort();

                match result {
                    Ok(Ok(())) => {
                        // Success — already reported by dispatch
                    }
                    Ok(Err(e)) => {
                        // dispatch returned an error (not a timeout)
                        error!("run {} failed: {e}", run.id);
                        let msg = format!("{e}");
                        let _ = service
                            .update_claude_run_status(&run.id, "failed", Some(&msg), None)
                            .await;
                    }
                    Err(_elapsed) => {
                        // TIMEOUT — dispatch didn't complete in time
                        warn!("run {} timed out after {:?}", run.id, timeout);
                        let _ = service
                            .update_claude_run_status(
                                &run.id,
                                "timed_out",
                                Some(&format!("timed out after {}s", timeout.as_secs())),
                                None,
                            )
                            .await;

                        // Attempt salvage for Build actions
                        if run.action == ClaudeAction::Build {
                            let ws_dir = executor::resolve_workspace_dir(
                                &config.workspace_root,
                                &run.id,
                            );
                            let outcome = salvage::attempt_salvage(
                                &service, &run, &task, &project, &ws_dir, &config,
                            )
                            .await;

                            match &outcome {
                                salvage::SalvageOutcome::PrCut { pr_url, pr_number } => {
                                    info!(
                                        "salvage succeeded: PR #{pr_number} at {pr_url}"
                                    );
                                }
                                salvage::SalvageOutcome::NothingToSalvage => {
                                    info!("salvage: nothing to salvage");
                                }
                                salvage::SalvageOutcome::ValidationFailed { error } => {
                                    warn!("salvage: validation failed: {error}");
                                }
                                salvage::SalvageOutcome::SalvageError { error } => {
                                    error!("salvage error: {error}");
                                }
                            }

                            // Clean up workspace after salvage
                            executor::cleanup_workspace(&ws_dir);
                        }
                    }
                }
            }
            Ok(None) => {
                // No work — sleep
            }
            Err(e) => {
                error!("claim failed: {e}");
            }
        }

        tokio::time::sleep(Duration::from_secs(config.poll_interval)).await;
    }
}

async fn heartbeat_loop(service: &HttpService, run_id: &str) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let _ = service.update_claude_run_progress(run_id, "heartbeat").await;
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
