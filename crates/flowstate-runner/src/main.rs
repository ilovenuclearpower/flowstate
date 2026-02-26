use std::net::SocketAddr;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

use anyhow::Result;
use axum::extract::State;
use axum::{routing::get, Json, Router};
use clap::Parser;
use flowstate_core::claude_run::ClaudeRun;
use flowstate_core::project::Project;
use flowstate_core::task::Task;
use flowstate_runner::backend::AgentBackend;
use flowstate_runner::config::{RunnerConfig, RuntimeConfig};
use flowstate_runner::run_tracker::{
    ActiveRun, ActiveRunSnapshot, RunOutcome, RunResult, RunTracker,
};
use flowstate_runner::{executor, preflight, salvage};
use flowstate_service::{HttpService, RunnerUtilization, TaskService};
use serde::Serialize;
use tokio::net::TcpListener;
use tokio::sync::Semaphore;
use tokio::task::JoinSet;
use tracing::{error, info, warn, Instrument};

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("info")),
        )
        .init();

    let config = RunnerConfig::parse();
    config.validate()?;

    // Build agent backend from configuration
    let backend = config.build_backend()?;
    let capability = config.capability()?;

    info!("flowstate-runner starting");
    info!("backend: {}", backend.name());
    if let Some(model) = backend.model_hint() {
        info!("model: {model}");
    }
    info!("capability: {capability}");
    info!("server: {}", config.server_url);
    info!(
        "timeouts: light={}s, build={}s, kill_grace={}s",
        config.light_timeout, config.build_timeout, config.kill_grace_period
    );
    info!(
        "concurrency: max_concurrent={}, max_builds={}, shutdown_timeout={}s",
        config.max_concurrent, config.max_builds, config.shutdown_timeout
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
    svc.set_runner_id(runner_id.clone());
    let service = Arc::new(svc);

    // Run preflight checks
    preflight::run_all(&service, backend.as_ref()).await?;

    // Register with the server
    if let Err(e) = service
        .register_runner(&runner_id, backend.name(), capability.as_str())
        .await
    {
        warn!("runner registration failed (non-fatal): {e}");
    } else {
        info!("registered with server");
    }

    let backend: Arc<dyn AgentBackend> = Arc::from(backend);

    // Create RuntimeConfig (dynamically adjustable by server)
    let runtime_config = Arc::new(RwLock::new(RuntimeConfig::from_config(&config)));
    let config = Arc::new(config);

    // Concurrency primitives
    let total_semaphore = Arc::new(Semaphore::new(config.max_concurrent));
    let build_semaphore = Arc::new(Semaphore::new(config.max_builds));
    let tracker = Arc::new(RwLock::new(RunTracker::new()));

    // Shutdown flag
    let shutdown = Arc::new(AtomicBool::new(false));
    let shutdown_flag = shutdown.clone();
    tokio::spawn(async move {
        tokio::signal::ctrl_c().await.ok();
        info!("received shutdown signal, draining active runs...");
        shutdown_flag.store(true, Ordering::SeqCst);
    });

    // Start health endpoint in background
    let health_state = HealthState {
        tracker: tracker.clone(),
        config: config.clone(),
        runner_id: runner_id.clone(),
        backend_name: backend.name().to_string(),
        model_hint: backend.model_hint().map(|s| s.to_string()),
        capability: capability.as_str().to_string(),
    };
    let health_port = config.health_port;
    tokio::spawn(async move {
        if let Err(e) = run_health_server(health_port, health_state).await {
            error!("health server failed: {e}");
        }
    });

    info!("health endpoint: http://127.0.0.1:{health_port}/health");
    info!("entering poll loop (interval: {}s)", config.poll_interval);

    // JoinSet for concurrent run tasks
    let mut join_set: JoinSet<RunResult> = JoinSet::new();

    // Poll loop
    loop {
        // A. Check shutdown
        if shutdown.load(Ordering::SeqCst) {
            break;
        }

        // A2. Heartbeat via register_runner (report utilization, receive pending config)
        let heartbeat_util = {
            let (active_count, active_builds) = {
                let trk = tracker.read().unwrap();
                (trk.active_count(), trk.active_build_count())
            };
            let rt = runtime_config.read().unwrap();
            let drain_status = if rt.drain {
                Some("draining".to_string())
            } else {
                None
            };
            RunnerUtilization {
                poll_interval: rt.poll_interval,
                max_concurrent: config.max_concurrent,
                max_builds: config.max_builds,
                active_count,
                active_builds,
                status: drain_status,
            }
        };

        match service
            .register_runner_with_utilization(
                &runner_id,
                backend.name(),
                capability.as_str(),
                Some(&heartbeat_util),
            )
            .await
        {
            Ok(resp) => {
                if let Some(pending) = resp.pending_config {
                    apply_pending_config(&runtime_config, &pending);
                }
            }
            Err(e) => {
                warn!("heartbeat registration failed: {e}");
            }
        }

        // A3. Check drain flag
        let (is_draining, current_poll_interval) = {
            let rt = runtime_config.read().unwrap();
            (rt.drain, rt.poll_interval)
        };

        if is_draining {
            if join_set.is_empty() {
                info!("drain complete: no active runs, reporting drained and exiting");
                let drained_util = RunnerUtilization {
                    poll_interval: current_poll_interval,
                    max_concurrent: config.max_concurrent,
                    max_builds: config.max_builds,
                    active_count: 0,
                    active_builds: 0,
                    status: Some("drained".to_string()),
                };
                let _ = service
                    .register_runner_with_utilization(
                        &runner_id,
                        backend.name(),
                        capability.as_str(),
                        Some(&drained_util),
                    )
                    .await;
                break;
            }
            // Draining but still have active runs — drain completed tasks, skip claim
            while let Some(result) = join_set.try_join_next() {
                match result {
                    Ok(run_result) => log_run_outcome(&run_result),
                    Err(join_err) => error!("run task error: {join_err}"),
                }
            }
            info!(
                "draining: {} active run(s) remaining, skipping claims",
                join_set.len()
            );
            tokio::time::sleep(Duration::from_secs(current_poll_interval)).await;
            continue;
        }

        // B. Drain completed tasks from JoinSet (non-blocking)
        while let Some(result) = join_set.try_join_next() {
            match result {
                Ok(run_result) => {
                    log_run_outcome(&run_result);
                }
                Err(join_err) => {
                    error!("run task error: {join_err}");
                }
            }
        }

        // C. Claim loop: while we have capacity, claim work
        while total_semaphore.available_permits() > 0 {
            match service.claim_claude_run().await {
                Ok(Some(run)) => {
                    info!(
                        run_id = %run.id,
                        action = %run.action,
                        task_id = %run.task_id,
                        active = tracker.read().unwrap().active_count(),
                        "claimed run, spawning"
                    );

                    // Check if Build and no build capacity
                    if RunnerConfig::is_build_action(run.action)
                        && build_semaphore.available_permits() == 0
                    {
                        warn!(
                            run_id = %run.id,
                            "build claimed but no build capacity, re-queuing"
                        );
                        let _ = service
                            .update_claude_run_status(&run.id, "queued", None, None)
                            .await;
                        break;
                    }

                    // Fetch task + project
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

                    // Spawn into JoinSet
                    let svc = service.clone();
                    let cfg = config.clone();
                    let ts = total_semaphore.clone();
                    let bs = build_semaphore.clone();
                    let trk = tracker.clone();
                    let be = backend.clone();
                    join_set.spawn(execute_run(svc, run, task, project, cfg, ts, bs, trk, be));
                }
                Ok(None) => break, // no work available
                Err(e) => {
                    error!("claim failed: {e}");
                    break;
                }
            }
        }

        // D. Sleep (use dynamic poll_interval from RuntimeConfig)
        let poll_interval = runtime_config.read().unwrap().poll_interval;
        tokio::time::sleep(Duration::from_secs(poll_interval)).await;
    }

    // Graceful shutdown: wait for active runs to complete
    let active_count = join_set.len();
    if active_count > 0 {
        info!(
            "waiting up to {}s for {} active run(s) to complete",
            config.shutdown_timeout, active_count
        );

        let drain_result =
            tokio::time::timeout(Duration::from_secs(config.shutdown_timeout), async {
                while let Some(result) = join_set.join_next().await {
                    match result {
                        Ok(run_result) => log_run_outcome(&run_result),
                        Err(join_err) => error!("run task error during shutdown: {join_err}"),
                    }
                }
            })
            .await;

        if drain_result.is_err() {
            warn!(
                "shutdown timeout elapsed, aborting {} remaining run(s)",
                join_set.len()
            );
            join_set.abort_all();
            // Collect abort results to ensure tasks are cleaned up
            while join_set.join_next().await.is_some() {}
        }
    }

    info!("runner stopped");
    Ok(())
}

/// Apply pending config from the server to our RuntimeConfig.
fn apply_pending_config(
    runtime_config: &Arc<RwLock<RuntimeConfig>>,
    pending: &flowstate_service::PendingConfigResponse,
) {
    let mut rt = runtime_config.write().unwrap();
    if let Some(interval) = pending.poll_interval {
        if interval != rt.poll_interval {
            info!("applying pending config: poll_interval {} -> {interval}", rt.poll_interval);
            rt.poll_interval = interval;
        }
    }
    if let Some(drain) = pending.drain {
        if drain && !rt.drain {
            info!("applying pending config: drain=true, will stop claiming new runs");
            rt.drain = true;
        }
    }
}

/// Execute a single run within a spawned task.
/// Acquires semaphore permits, runs dispatch with timeout, handles results.
#[allow(clippy::too_many_arguments)]
async fn execute_run(
    service: Arc<HttpService>,
    run: ClaudeRun,
    task: Task,
    project: Project,
    config: Arc<RunnerConfig>,
    total_semaphore: Arc<Semaphore>,
    build_semaphore: Arc<Semaphore>,
    tracker: Arc<RwLock<RunTracker>>,
    backend: Arc<dyn AgentBackend>,
) -> RunResult {
    let run_id = run.id.clone();
    let task_id = run.task_id.clone();
    let action = run.action;

    let span = tracing::info_span!("run",
        run_id = %run.id,
        task_id = %run.task_id,
        action = %run.action,
    );

    async move {
        // Acquire total permit (should succeed immediately since we checked availability)
        let _total_permit = total_semaphore.acquire().await.unwrap();

        // Acquire build permit if needed
        let _build_permit = if RunnerConfig::is_build_action(action) {
            Some(build_semaphore.acquire().await.unwrap())
        } else {
            None
        };

        // Register in tracker
        tracker.write().unwrap().insert(ActiveRun {
            run_id: run_id.clone(),
            task_id: task_id.clone(),
            action,
            started_at: Instant::now(),
        });

        let timeout = config.timeout_for_action(action);

        // Start heartbeat
        let heartbeat_service = service.clone();
        let heartbeat_run_id = run_id.clone();
        let heartbeat = tokio::spawn(async move {
            heartbeat_loop(&heartbeat_service, &heartbeat_run_id).await;
        });

        // Execute with timeout
        let result = tokio::time::timeout(
            timeout,
            executor::dispatch(&service, &run, &task, &project, &config, backend.as_ref()),
        )
        .await;

        // Stop heartbeat
        heartbeat.abort();

        let outcome = match result {
            Ok(Ok(())) => {
                // Success — already reported by dispatch
                RunOutcome::Success
            }
            Ok(Err(e)) => {
                // dispatch returned an error (not a timeout)
                error!("run failed: {e}");
                let msg = format!("{e}");
                let _ = service
                    .update_claude_run_status(&run_id, "failed", Some(&msg), None)
                    .await;
                RunOutcome::Failed(msg)
            }
            Err(_elapsed) => {
                // TIMEOUT — dispatch didn't complete in time
                warn!("run timed out after {:?}", timeout);
                let _ = service
                    .update_claude_run_status(
                        &run_id,
                        "timed_out",
                        Some(&format!("timed out after {}s", timeout.as_secs())),
                        None,
                    )
                    .await;

                // Attempt salvage for Build actions (under the same build permit)
                if RunnerConfig::is_build_action(action) {
                    let ws_dir = executor::resolve_workspace_dir(&config.workspace_root, &run_id);
                    let outcome =
                        salvage::attempt_salvage(&service, &run, &task, &project, &ws_dir, &config)
                            .await;

                    match &outcome {
                        salvage::SalvageOutcome::PrCut { pr_url, pr_number } => {
                            info!("salvage succeeded: PR #{pr_number} at {pr_url}");
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

                RunOutcome::TimedOut
            }
        };

        // Unregister from tracker
        tracker.write().unwrap().remove(&run_id);

        RunResult {
            run_id,
            task_id,
            action,
            outcome,
        }
    }
    .instrument(span)
    .await
}

fn log_run_outcome(result: &RunResult) {
    match &result.outcome {
        RunOutcome::Success => {
            info!(
                run_id = %result.run_id,
                action = %result.action,
                "run completed successfully"
            );
        }
        RunOutcome::Failed(msg) => {
            error!(
                run_id = %result.run_id,
                action = %result.action,
                error = %msg,
                "run failed"
            );
        }
        RunOutcome::TimedOut => {
            warn!(
                run_id = %result.run_id,
                action = %result.action,
                "run timed out"
            );
        }
        RunOutcome::Panicked(msg) => {
            error!(
                run_id = %result.run_id,
                action = %result.action,
                error = %msg,
                "run panicked"
            );
        }
    }
}

async fn heartbeat_loop(service: &HttpService, run_id: &str) {
    let mut interval = tokio::time::interval(Duration::from_secs(30));
    loop {
        interval.tick().await;
        let _ = service
            .update_claude_run_progress(run_id, "heartbeat")
            .await;
    }
}

// --- Health endpoint ---

#[derive(Clone)]
struct HealthState {
    tracker: Arc<RwLock<RunTracker>>,
    config: Arc<RunnerConfig>,
    runner_id: String,
    backend_name: String,
    model_hint: Option<String>,
    capability: String,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    role: &'static str,
    runner_id: String,
    backend: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    model: Option<String>,
    capability: String,
    capacity: CapacityInfo,
    active_runs: Vec<ActiveRunSnapshot>,
}

#[derive(Serialize)]
struct CapacityInfo {
    max_concurrent: usize,
    max_builds: usize,
    active_total: usize,
    active_builds: usize,
    available: usize,
}

async fn health_handler(State(state): State<HealthState>) -> Json<HealthResponse> {
    let tracker = state.tracker.read().unwrap();
    let active_total = tracker.active_count();
    let active_builds = tracker.active_build_count();
    let active_runs = tracker.snapshot();
    drop(tracker);

    Json(HealthResponse {
        status: "ok",
        role: "runner",
        runner_id: state.runner_id.clone(),
        backend: state.backend_name.clone(),
        model: state.model_hint.clone(),
        capability: state.capability.clone(),
        capacity: CapacityInfo {
            max_concurrent: state.config.max_concurrent,
            max_builds: state.config.max_builds,
            active_total,
            active_builds,
            available: state.config.max_concurrent.saturating_sub(active_total),
        },
        active_runs,
    })
}

async fn run_health_server(port: u16, state: HealthState) -> Result<()> {
    let app = Router::new()
        .route("/health", get(health_handler))
        .with_state(state);

    let addr = SocketAddr::from(([127, 0, 0, 1], port));
    let listener = TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}
