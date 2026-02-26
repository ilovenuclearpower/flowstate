use std::sync::Arc;
use std::time::Instant;

use serde::Serialize;
use tokio::sync::Mutex as TokioMutex;
use tracing::{error, info, warn};

use crate::routes::{AppState, PendingConfig, RunnerStatus};

// ---------------------------------------------------------------------------
// RunPod API trait (for testability)
// ---------------------------------------------------------------------------

/// Abstraction over the RunPod HTTP API.
#[async_trait::async_trait]
pub trait RunPodApi: Send + Sync {
    async fn get_pod(&self, pod_id: &str) -> Result<PodInfo, PodApiError>;
    async fn start_pod(&self, pod_id: &str) -> Result<(), PodApiError>;
    async fn stop_pod(&self, pod_id: &str) -> Result<(), PodApiError>;
    async fn create_pod(&self, config: &PodCreateRequest) -> Result<String, PodApiError>;
}

#[derive(Debug)]
pub struct PodApiError(pub String);

impl std::fmt::Display for PodApiError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "RunPod API error: {}", self.0)
    }
}

#[derive(Debug, Clone)]
pub struct PodInfo {
    pub id: String,
    pub status: String,
    pub gpu_type: Option<String>,
    pub cost_per_hr: Option<f64>,
}

#[derive(Debug, Clone)]
pub struct PodCreateRequest {
    pub name: String,
    pub image: String,
    pub gpu_type: String,
    pub gpu_count: u32,
    pub cloud_type: String,
    pub network_volume_id: Option<String>,
    pub env_vars: Vec<(String, String)>,
}

// ---------------------------------------------------------------------------
// Pod Manager Configuration (from env vars)
// ---------------------------------------------------------------------------

/// Configuration for the pod manager. Read from environment variables.
/// Disabled if `FLOWSTATE_RUNPOD_API_KEY` is not set.
#[derive(Debug, Clone)]
pub struct PodManagerConfig {
    pub api_key: String,
    pub pod_id: Option<String>,
    pub template_image: String,
    pub gpu_type: String,
    pub gpu_count: u32,
    pub network_volume: Option<String>,
    pub idle_timeout_secs: u64,
    pub queue_threshold: i64,
    pub scan_interval_secs: u64,
    pub max_daily_spend_cents: u64,
    pub spindown_threshold: i64,
    pub drain_timeout_secs: u64,
    pub cloud_type: String,
    /// Environment variables passed to the RunPod pod when created.
    /// Assembled from `FLOWSTATE_RUNPOD_POD_*` env vars on the server.
    pub pod_env: Vec<(String, String)>,
    pub ts_authkey: Option<String>,
}

impl PodManagerConfig {
    /// Build from environment variables. Returns None if `FLOWSTATE_RUNPOD_API_KEY` is not set.
    pub fn from_env() -> Option<Self> {
        let api_key = std::env::var("FLOWSTATE_RUNPOD_API_KEY").ok()?;
        let ts_authkey = std::env::var("FLOWSTATE_RUNPOD_TS_AUTHKEY").ok();

        // Assemble environment variables to pass to the RunPod pod.
        // Server-side FLOWSTATE_RUNPOD_POD_* vars are mapped to the pod's env.
        let pod_env = Self::build_pod_env(&ts_authkey);

        Some(Self {
            api_key,
            pod_id: std::env::var("FLOWSTATE_RUNPOD_POD_ID").ok(),
            template_image: std::env::var("FLOWSTATE_RUNPOD_TEMPLATE_IMAGE").unwrap_or_else(|_| {
                "ghcr.io/ilovenuclearpower/flowstate-runner-gpu-tailscale:latest".into()
            }),
            gpu_type: std::env::var("FLOWSTATE_RUNPOD_GPU_TYPE")
                .unwrap_or_else(|_| "NVIDIA RTX A5000".into()),
            gpu_count: std::env::var("FLOWSTATE_RUNPOD_GPU_COUNT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            network_volume: std::env::var("FLOWSTATE_RUNPOD_NETWORK_VOLUME").ok(),
            idle_timeout_secs: std::env::var("FLOWSTATE_RUNPOD_IDLE_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(300),
            queue_threshold: std::env::var("FLOWSTATE_RUNPOD_QUEUE_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(1),
            scan_interval_secs: std::env::var("FLOWSTATE_RUNPOD_SCAN_INTERVAL")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(30),
            max_daily_spend_cents: std::env::var("FLOWSTATE_RUNPOD_MAX_DAILY_SPEND")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(5000), // $50 default
            spindown_threshold: std::env::var("FLOWSTATE_RUNPOD_SPINDOWN_THRESHOLD")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(0),
            drain_timeout_secs: std::env::var("FLOWSTATE_RUNPOD_DRAIN_TIMEOUT")
                .ok()
                .and_then(|s| s.parse().ok())
                .unwrap_or(600),
            cloud_type: std::env::var("FLOWSTATE_RUNPOD_CLOUD_TYPE")
                .unwrap_or_else(|_| "COMMUNITY".into()),
            pod_env,
            ts_authkey,
        })
    }

    /// Build the environment variables that will be injected into the RunPod pod.
    ///
    /// These are read from server-side `FLOWSTATE_RUNPOD_POD_*` env vars and mapped
    /// to the names the entrypoint.sh / runner expect inside the container:
    ///
    /// | Server env var                     | Pod env var                  |
    /// |------------------------------------|------------------------------|
    /// | `FLOWSTATE_RUNPOD_TS_AUTHKEY`      | `TS_AUTHKEY`                 |
    /// | `FLOWSTATE_RUNPOD_POD_SERVER_IP`   | `TS_SERVER_IP`               |
    /// | `FLOWSTATE_RUNPOD_POD_SERVER_URL`  | `FLOWSTATE_SERVER_URL`       |
    /// | `FLOWSTATE_RUNPOD_POD_API_KEY`     | `FLOWSTATE_API_KEY`          |
    /// | `FLOWSTATE_RUNPOD_POD_CAPABILITY`  | `FLOWSTATE_RUNNER_CAPABILITY`|
    /// | `FLOWSTATE_RUNPOD_POD_BACKEND`     | `FLOWSTATE_AGENT_BACKEND`    |
    /// | `FLOWSTATE_RUNPOD_POD_VLLM_MODEL`  | `VLLM_MODEL`                |
    /// | `FLOWSTATE_RUNPOD_POD_VLLM_MAX_MODEL_LEN` | `VLLM_MAX_MODEL_LEN` |
    /// | `FLOWSTATE_RUNPOD_POD_MAX_CONCURRENT` | `FLOWSTATE_MAX_CONCURRENT`|
    /// | `FLOWSTATE_RUNPOD_POD_MAX_BUILDS`  | `FLOWSTATE_MAX_BUILDS`       |
    /// | `FLOWSTATE_RUNPOD_POD_HF_TOKEN`    | `HF_TOKEN`                   |
    fn build_pod_env(ts_authkey: &Option<String>) -> Vec<(String, String)> {
        let mut env = Vec::new();

        // Tailscale
        if let Some(key) = ts_authkey {
            env.push(("TS_AUTHKEY".into(), key.clone()));
        }
        if let Ok(ip) = std::env::var("FLOWSTATE_RUNPOD_POD_SERVER_IP") {
            env.push(("TS_SERVER_IP".into(), ip));
        }

        // Flowstate runner connection
        if let Ok(url) = std::env::var("FLOWSTATE_RUNPOD_POD_SERVER_URL") {
            env.push(("FLOWSTATE_SERVER_URL".into(), url));
        }
        if let Ok(key) = std::env::var("FLOWSTATE_RUNPOD_POD_API_KEY") {
            env.push(("FLOWSTATE_API_KEY".into(), key));
        }

        // Runner configuration
        if let Ok(cap) = std::env::var("FLOWSTATE_RUNPOD_POD_CAPABILITY") {
            env.push(("FLOWSTATE_RUNNER_CAPABILITY".into(), cap));
        }
        if let Ok(backend) = std::env::var("FLOWSTATE_RUNPOD_POD_BACKEND") {
            env.push(("FLOWSTATE_AGENT_BACKEND".into(), backend));
        }
        if let Ok(mc) = std::env::var("FLOWSTATE_RUNPOD_POD_MAX_CONCURRENT") {
            env.push(("FLOWSTATE_MAX_CONCURRENT".into(), mc));
        }
        if let Ok(mb) = std::env::var("FLOWSTATE_RUNPOD_POD_MAX_BUILDS") {
            env.push(("FLOWSTATE_MAX_BUILDS".into(), mb));
        }

        // vLLM
        if let Ok(model) = std::env::var("FLOWSTATE_RUNPOD_POD_VLLM_MODEL") {
            env.push(("VLLM_MODEL".into(), model));
        }
        if let Ok(len) = std::env::var("FLOWSTATE_RUNPOD_POD_VLLM_MAX_MODEL_LEN") {
            env.push(("VLLM_MAX_MODEL_LEN".into(), len));
        }

        // HuggingFace
        if let Ok(token) = std::env::var("FLOWSTATE_RUNPOD_POD_HF_TOKEN") {
            env.push(("HF_TOKEN".into(), token));
        }

        env
    }
}

// ---------------------------------------------------------------------------
// Pod Manager State
// ---------------------------------------------------------------------------

/// The pod manager's in-memory state.
#[derive(Debug, Clone, Serialize)]
pub struct PodManagerState {
    /// The RunPod pod ID (if known).
    pub pod_id: Option<String>,
    /// Current pod status as known to the manager.
    pub pod_status: PodStatus,
    /// When work was last seen in the queue.
    #[serde(skip)]
    pub last_work_seen: Option<Instant>,
    /// Daily cost accumulator in cents.
    pub daily_cost_cents: u64,
    /// When the current cost-tracking day started.
    #[serde(skip)]
    pub day_start: Instant,
    /// Whether the cost cap has been hit today.
    pub cost_capped: bool,
    /// When a drain was requested.
    #[serde(skip)]
    pub drain_requested_at: Option<Instant>,
}

/// Pod lifecycle status from the pod manager's perspective.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PodStatus {
    Unknown,
    Stopped,
    Starting,
    Running,
    Draining,
    Drained,
}

impl PodManagerState {
    pub fn new(pod_id: Option<String>) -> Self {
        Self {
            pod_id,
            pod_status: PodStatus::Unknown,
            last_work_seen: None,
            daily_cost_cents: 0,
            day_start: Instant::now(),
            cost_capped: false,
            drain_requested_at: None,
        }
    }
}

// ---------------------------------------------------------------------------
// Pod Manager Loop
// ---------------------------------------------------------------------------

/// Run the pod manager decision loop. This runs as a background task
/// within the server process.
pub async fn run_pod_manager(
    state: AppState,
    config: PodManagerConfig,
    pod_state: Arc<TokioMutex<PodManagerState>>,
    api: Arc<dyn RunPodApi>,
) {
    let interval = std::time::Duration::from_secs(config.scan_interval_secs);
    info!(
        "pod manager started (scan_interval={}s, queue_threshold={}, idle_timeout={}s)",
        config.scan_interval_secs, config.queue_threshold, config.idle_timeout_secs
    );

    loop {
        tokio::time::sleep(interval).await;

        if let Err(e) = pod_manager_tick(&state, &config, &pod_state, api.as_ref()).await {
            error!("pod manager tick error: {e}");
        }
    }
}

/// Single tick of the pod manager decision loop.
pub async fn pod_manager_tick(
    state: &AppState,
    config: &PodManagerConfig,
    pod_state: &Arc<TokioMutex<PodManagerState>>,
    api: &dyn RunPodApi,
) -> Result<(), String> {
    // 1. Get queue depth
    let queue_depth = state
        .db
        .count_queued_runs()
        .await
        .map_err(|e| format!("count_queued_runs: {e}"))?;

    let mut ps = pod_state.lock().await;

    // Reset daily cost if a new day has started (24h window)
    if ps.day_start.elapsed().as_secs() > 86400 {
        ps.daily_cost_cents = 0;
        ps.day_start = Instant::now();
        ps.cost_capped = false;
        info!("pod manager: daily cost reset");
    }

    // 2. Get real pod status if we have a pod_id
    if let Some(ref pod_id) = ps.pod_id {
        match api.get_pod(pod_id).await {
            Ok(info) => {
                let new_status = match info.status.as_str() {
                    "RUNNING" => PodStatus::Running,
                    "EXITED" | "STOPPED" | "TERMINATED" => PodStatus::Stopped,
                    "CREATED" | "STARTING" => PodStatus::Starting,
                    _ => PodStatus::Unknown,
                };
                // Don't override Draining/Drained status from our side
                if ps.pod_status != PodStatus::Draining && ps.pod_status != PodStatus::Drained {
                    ps.pod_status = new_status;
                }

                // Accumulate cost
                if let Some(cost_per_hr) = info.cost_per_hr {
                    let cost_per_tick =
                        (cost_per_hr * 100.0 * config.scan_interval_secs as f64 / 3600.0) as u64;
                    ps.daily_cost_cents += cost_per_tick;
                }
            }
            Err(e) => {
                warn!("pod manager: failed to get pod status: {e}");
            }
        }
    }

    // 3. Find the RunPod runner in the runners map
    let runner_id = find_runpod_runner(&state.runners);

    // 4. Decision logic

    // COST CAP: if daily spend exceeds max, drain and stop
    if ps.daily_cost_cents > config.max_daily_spend_cents && !ps.cost_capped {
        warn!(
            "pod manager: daily cost cap reached ({} cents > {} cents), draining",
            ps.daily_cost_cents, config.max_daily_spend_cents
        );
        ps.cost_capped = true;
        if let Some(ref rid) = runner_id {
            set_runner_drain(&state.runners, rid);
        }
        ps.pod_status = PodStatus::Draining;
        ps.drain_requested_at = Some(Instant::now());
        return Ok(());
    }

    match ps.pod_status {
        PodStatus::Stopped | PodStatus::Unknown => {
            // SPIN UP: if queued >= threshold and not cost-capped
            if queue_depth >= config.queue_threshold && !ps.cost_capped {
                info!(
                    "pod manager: queue_depth={queue_depth} >= threshold={}, spinning up",
                    config.queue_threshold
                );
                if let Some(ref pod_id) = ps.pod_id {
                    if let Err(e) = api.start_pod(pod_id).await {
                        error!("pod manager: failed to start pod: {e}");
                    } else {
                        ps.pod_status = PodStatus::Starting;
                    }
                } else {
                    // Create a new pod
                    let req = PodCreateRequest {
                        name: "flowstate-gpu".into(),
                        image: config.template_image.clone(),
                        gpu_type: config.gpu_type.clone(),
                        gpu_count: config.gpu_count,
                        cloud_type: config.cloud_type.clone(),
                        network_volume_id: config.network_volume.clone(),
                        env_vars: config.pod_env.clone(),
                    };
                    match api.create_pod(&req).await {
                        Ok(new_id) => {
                            info!("pod manager: created pod {new_id}");
                            ps.pod_id = Some(new_id);
                            ps.pod_status = PodStatus::Starting;
                        }
                        Err(e) => {
                            error!("pod manager: failed to create pod: {e}");
                        }
                    }
                }
            }
        }
        PodStatus::Starting => {
            // Nothing to do, wait for it to become Running
        }
        PodStatus::Running => {
            if queue_depth > 0 {
                ps.last_work_seen = Some(Instant::now());
            }

            // DRAIN: pod running, queue low, idle too long
            let idle_secs = ps
                .last_work_seen
                .map(|t| t.elapsed().as_secs())
                .unwrap_or(config.idle_timeout_secs + 1);

            if queue_depth <= config.spindown_threshold && idle_secs > config.idle_timeout_secs {
                info!("pod manager: idle for {idle_secs}s, draining");
                if let Some(ref rid) = runner_id {
                    set_runner_drain(&state.runners, rid);
                }
                ps.pod_status = PodStatus::Draining;
                ps.drain_requested_at = Some(Instant::now());
            }
        }
        PodStatus::Draining => {
            // Check if runner has reported drained
            let is_drained = runner_id
                .as_ref()
                .map(|rid| {
                    let runners = state.runners.lock().unwrap();
                    runners
                        .get(rid)
                        .map(|r| r.status == RunnerStatus::Drained)
                        .unwrap_or(false)
                })
                .unwrap_or(false);

            if is_drained {
                info!("pod manager: runner drained, stopping pod");
                if let Some(ref pod_id) = ps.pod_id {
                    if let Err(e) = api.stop_pod(pod_id).await {
                        error!("pod manager: failed to stop pod: {e}");
                    }
                }
                ps.pod_status = PodStatus::Stopped;
                ps.drain_requested_at = None;
            } else if let Some(drain_start) = ps.drain_requested_at {
                // DRAIN TIMEOUT: force stop if draining too long
                if drain_start.elapsed().as_secs() > config.drain_timeout_secs {
                    warn!("pod manager: drain timeout, force stopping pod");
                    if let Some(ref pod_id) = ps.pod_id {
                        if let Err(e) = api.stop_pod(pod_id).await {
                            error!("pod manager: failed to force stop pod: {e}");
                        }
                    }
                    ps.pod_status = PodStatus::Stopped;
                    ps.drain_requested_at = None;
                }
            }
        }
        PodStatus::Drained => {
            // Should have been handled, stop pod
            if let Some(ref pod_id) = ps.pod_id {
                if let Err(e) = api.stop_pod(pod_id).await {
                    error!("pod manager: failed to stop drained pod: {e}");
                }
            }
            ps.pod_status = PodStatus::Stopped;
        }
    }

    Ok(())
}

/// Find a runner that looks like a RunPod runner (heuristic: check for known naming patterns).
/// For now, returns the first runner whose runner_id is not "unknown".
fn find_runpod_runner(
    runners: &std::sync::Mutex<std::collections::HashMap<String, crate::routes::RunnerInfo>>,
) -> Option<String> {
    let runners = runners.lock().unwrap();
    // In practice, the RunPod runner will be the only runner or the one with
    // a specific naming convention. For now, return the first registered runner.
    runners.keys().next().cloned()
}

/// Set drain pending_config on a specific runner.
fn set_runner_drain(
    runners: &std::sync::Mutex<std::collections::HashMap<String, crate::routes::RunnerInfo>>,
    runner_id: &str,
) {
    let mut runners = runners.lock().unwrap();
    if let Some(info) = runners.get_mut(runner_id) {
        info.pending_config = Some(PendingConfig {
            poll_interval: None,
            drain: Some(true),
        });
    }
}

// ---------------------------------------------------------------------------
// Real RunPod API Client (wraps the `runpod` crate)
// ---------------------------------------------------------------------------

pub struct RunPodClient {
    inner: runpod::RunpodClient,
}

impl RunPodClient {
    pub fn new(api_key: &str) -> Self {
        Self {
            inner: runpod::RunpodClient::new(api_key),
        }
    }
}

#[async_trait::async_trait]
impl RunPodApi for RunPodClient {
    async fn get_pod(&self, pod_id: &str) -> Result<PodInfo, PodApiError> {
        let resp = self
            .inner
            .get_pod(pod_id)
            .await
            .map_err(|e| PodApiError(e.to_string()))?;

        let pod = resp
            .data
            .ok_or_else(|| PodApiError("missing pod data in response".into()))?;

        Ok(PodInfo {
            id: pod.id,
            status: pod.desired_status,
            gpu_type: None, // PodInfoFull doesn't expose gpu_type directly
            cost_per_hr: Some(pod.cost_per_hr),
        })
    }

    async fn start_pod(&self, pod_id: &str) -> Result<(), PodApiError> {
        let resp = self
            .inner
            .start_pod(pod_id)
            .await
            .map_err(|e| PodApiError(e.to_string()))?;

        if let Some(errors) = resp.errors {
            if !errors.is_empty() {
                return Err(PodApiError(format!("start_pod errors: {:?}", errors)));
            }
        }
        Ok(())
    }

    async fn stop_pod(&self, pod_id: &str) -> Result<(), PodApiError> {
        let resp = self
            .inner
            .stop_pod(pod_id)
            .await
            .map_err(|e| PodApiError(e.to_string()))?;

        if let Some(errors) = resp.errors {
            if !errors.is_empty() {
                return Err(PodApiError(format!("stop_pod errors: {:?}", errors)));
            }
        }
        Ok(())
    }

    async fn create_pod(&self, config: &PodCreateRequest) -> Result<String, PodApiError> {
        let env: Vec<runpod::EnvVar> = config
            .env_vars
            .iter()
            .map(|(k, v)| runpod::EnvVar {
                key: k.clone(),
                value: v.clone(),
            })
            .collect();

        let req = runpod::CreateOnDemandPodRequest {
            name: Some(config.name.clone()),
            image_name: Some(config.image.clone()),
            gpu_type_id: Some(config.gpu_type.clone()),
            gpu_count: Some(config.gpu_count as i32),
            cloud_type: Some(config.cloud_type.clone()),
            volume_in_gb: Some(50),
            container_disk_in_gb: Some(50),
            network_volume_id: config.network_volume_id.clone(),
            env,
            ..Default::default()
        };

        let resp = self
            .inner
            .create_on_demand_pod(req)
            .await
            .map_err(|e| PodApiError(e.to_string()))?;

        let pod = resp
            .data
            .ok_or_else(|| PodApiError("missing pod data in create response".into()))?;

        Ok(pod.id)
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::routes::{InnerAppState, RunnerInfo, RunnerStatus};
    use flowstate_service::LocalService;
    use std::collections::HashMap;
    use std::sync::Arc;

    /// Mock RunPod API for testing.
    struct MockRunPodApi {
        pod_status: std::sync::Mutex<String>,
        started: std::sync::Mutex<bool>,
        stopped: std::sync::Mutex<bool>,
    }

    impl MockRunPodApi {
        fn new(status: &str) -> Self {
            Self {
                pod_status: std::sync::Mutex::new(status.into()),
                started: std::sync::Mutex::new(false),
                stopped: std::sync::Mutex::new(false),
            }
        }
    }

    #[async_trait::async_trait]
    impl RunPodApi for MockRunPodApi {
        async fn get_pod(&self, pod_id: &str) -> Result<PodInfo, PodApiError> {
            Ok(PodInfo {
                id: pod_id.to_string(),
                status: self.pod_status.lock().unwrap().clone(),
                gpu_type: Some("NVIDIA RTX A5000".into()),
                cost_per_hr: Some(0.50),
            })
        }

        async fn start_pod(&self, _pod_id: &str) -> Result<(), PodApiError> {
            *self.started.lock().unwrap() = true;
            *self.pod_status.lock().unwrap() = "RUNNING".into();
            Ok(())
        }

        async fn stop_pod(&self, _pod_id: &str) -> Result<(), PodApiError> {
            *self.stopped.lock().unwrap() = true;
            *self.pod_status.lock().unwrap() = "EXITED".into();
            Ok(())
        }

        async fn create_pod(&self, _config: &PodCreateRequest) -> Result<String, PodApiError> {
            Ok("new-pod-123".into())
        }
    }

    fn test_config() -> PodManagerConfig {
        PodManagerConfig {
            api_key: "test-key".into(),
            pod_id: Some("pod-1".into()),
            template_image: "test-image".into(),
            gpu_type: "NVIDIA RTX A5000".into(),
            gpu_count: 1,
            network_volume: None,
            idle_timeout_secs: 300,
            queue_threshold: 1,
            scan_interval_secs: 30,
            max_daily_spend_cents: 5000,
            spindown_threshold: 0,
            drain_timeout_secs: 600,
            cloud_type: "COMMUNITY".into(),
            pod_env: vec![],
            ts_authkey: None,
        }
    }

    async fn test_state() -> AppState {
        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
        let service = LocalService::new(db.clone());
        let store_config = flowstate_store::StoreConfig {
            endpoint_url: None,
            region: None,
            bucket: None,
            access_key_id: None,
            secret_access_key: None,
            local_data_dir: Some(
                tempfile::tempdir()
                    .unwrap()
                    .keep()
                    .to_string_lossy()
                    .to_string(),
            ),
        };
        let store = flowstate_store::create_store(&store_config).unwrap();
        use aes_gcm::KeyInit;
        let key = aes_gcm::Aes256Gcm::generate_key(aes_gcm::aead::OsRng);
        Arc::new(InnerAppState {
            service,
            db,
            auth: None,
            runners: std::sync::Mutex::new(HashMap::new()),
            encryption_key: key,
            store,
            pod_manager: None,
        })
    }

    #[tokio::test]
    async fn test_spin_up_when_queue_has_work() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("EXITED"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));

        // Set pod to stopped
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Stopped;
        }

        // Create a queued run in the DB
        let project = state
            .db
            .create_project(&flowstate_core::project::CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = state
            .db
            .create_task(&flowstate_core::task::CreateTask {
                project_id: project.id,
                title: "T".into(),
                description: String::new(),
                status: flowstate_core::task::Status::Todo,
                priority: flowstate_core::task::Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();
        state
            .db
            .create_claude_run(&flowstate_core::claude_run::CreateClaudeRun {
                task_id: task.id,
                action: flowstate_core::claude_run::ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        // Tick
        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        assert!(*api.started.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Starting);
    }

    #[tokio::test]
    async fn test_no_spin_up_when_queue_empty() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("EXITED"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Stopped;
        }

        // No queued runs
        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        assert!(!*api.started.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Stopped);
    }

    #[tokio::test]
    async fn test_drain_when_idle() {
        let state = test_state().await;
        let mut config = test_config();
        config.idle_timeout_secs = 0; // instant drain
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Running;
            // last_work_seen is None => idle_secs will exceed timeout
        }

        // Register a runner so drain can be set
        {
            let mut runners = state.runners.lock().unwrap();
            runners.insert(
                "runner-1".into(),
                RunnerInfo {
                    runner_id: "runner-1".into(),
                    last_seen: chrono::Utc::now(),
                    backend_name: None,
                    capability: None,
                    capabilities: vec![],
                    poll_interval: None,
                    max_concurrent: None,
                    max_builds: None,
                    active_count: None,
                    active_builds: None,
                    status: RunnerStatus::Active,
                    pending_config: None,
                },
            );
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Draining);
        assert!(ps.drain_requested_at.is_some());

        // Check runner got pending drain
        let runners = state.runners.lock().unwrap();
        let runner = runners.get("runner-1").unwrap();
        assert_eq!(runner.pending_config.as_ref().unwrap().drain, Some(true));
    }

    #[tokio::test]
    async fn test_drain_complete_stops_pod() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Draining;
            ps.drain_requested_at = Some(Instant::now());
        }

        // Register a runner in Drained state
        {
            let mut runners = state.runners.lock().unwrap();
            runners.insert(
                "runner-1".into(),
                RunnerInfo {
                    runner_id: "runner-1".into(),
                    last_seen: chrono::Utc::now(),
                    backend_name: None,
                    capability: None,
                    capabilities: vec![],
                    poll_interval: None,
                    max_concurrent: None,
                    max_builds: None,
                    active_count: None,
                    active_builds: None,
                    status: RunnerStatus::Drained,
                    pending_config: None,
                },
            );
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        assert!(*api.stopped.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Stopped);
    }

    #[tokio::test]
    async fn test_drain_timeout_force_stops() {
        let state = test_state().await;
        let mut config = test_config();
        config.drain_timeout_secs = 0; // instant timeout
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Draining;
            ps.drain_requested_at = Some(Instant::now() - std::time::Duration::from_secs(1));
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        assert!(*api.stopped.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Stopped);
    }

    #[tokio::test]
    async fn test_create_pod_when_no_pod_id() {
        let state = test_state().await;
        let mut config = test_config();
        config.pod_id = None; // No existing pod
        let api = Arc::new(MockRunPodApi::new("EXITED"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(None)));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Stopped;
        }

        // Create a queued run
        let project = state
            .db
            .create_project(&flowstate_core::project::CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = state
            .db
            .create_task(&flowstate_core::task::CreateTask {
                project_id: project.id,
                title: "T".into(),
                description: String::new(),
                status: flowstate_core::task::Status::Todo,
                priority: flowstate_core::task::Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();
        state
            .db
            .create_claude_run(&flowstate_core::claude_run::CreateClaudeRun {
                task_id: task.id,
                action: flowstate_core::claude_run::ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        // Should have created a new pod, not started an existing one
        assert!(!*api.started.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_id, Some("new-pod-123".into()));
        assert_eq!(ps.pod_status, PodStatus::Starting);
    }

    #[tokio::test]
    async fn test_stay_warm_resets_last_work_seen() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Running;
            ps.last_work_seen = None;
        }

        // Create a queued run so queue_depth > 0
        let project = state
            .db
            .create_project(&flowstate_core::project::CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = state
            .db
            .create_task(&flowstate_core::task::CreateTask {
                project_id: project.id,
                title: "T".into(),
                description: String::new(),
                status: flowstate_core::task::Status::Todo,
                priority: flowstate_core::task::Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();
        state
            .db
            .create_claude_run(&flowstate_core::claude_run::CreateClaudeRun {
                task_id: task.id,
                action: flowstate_core::claude_run::ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        let ps = pod_state.lock().await;
        // last_work_seen should have been set
        assert!(ps.last_work_seen.is_some());
        // Should still be running (not draining — there's work)
        assert_eq!(ps.pod_status, PodStatus::Running);
    }

    #[tokio::test]
    async fn test_drained_status_stops_pod() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Drained;
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        assert!(*api.stopped.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Stopped);
    }

    #[tokio::test]
    async fn test_cost_capped_prevents_spin_up() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("EXITED"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Stopped;
            ps.cost_capped = true;
        }

        // Create a queued run
        let project = state
            .db
            .create_project(&flowstate_core::project::CreateProject {
                name: "P".into(),
                slug: "p".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();
        let task = state
            .db
            .create_task(&flowstate_core::task::CreateTask {
                project_id: project.id,
                title: "T".into(),
                description: String::new(),
                status: flowstate_core::task::Status::Todo,
                priority: flowstate_core::task::Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();
        state
            .db
            .create_claude_run(&flowstate_core::claude_run::CreateClaudeRun {
                task_id: task.id,
                action: flowstate_core::claude_run::ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        // Should NOT have started despite queued work
        assert!(!*api.started.lock().unwrap());
        let ps = pod_state.lock().await;
        assert_eq!(ps.pod_status, PodStatus::Stopped);
    }

    #[tokio::test]
    async fn test_daily_cost_reset() {
        let state = test_state().await;
        let config = test_config();
        let api = Arc::new(MockRunPodApi::new("EXITED"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Stopped;
            ps.daily_cost_cents = 9999;
            ps.cost_capped = true;
            // Set day_start far enough in the past to trigger reset
            ps.day_start = Instant::now() - std::time::Duration::from_secs(86401);
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        let ps = pod_state.lock().await;
        assert_eq!(ps.daily_cost_cents, 0);
        assert!(!ps.cost_capped);
    }

    #[tokio::test]
    async fn test_get_pod_error_continues() {
        let state = test_state().await;
        let config = test_config();

        // Create a mock that returns an error from get_pod
        struct FailGetPodApi;

        #[async_trait::async_trait]
        impl RunPodApi for FailGetPodApi {
            async fn get_pod(&self, _pod_id: &str) -> Result<PodInfo, PodApiError> {
                Err(PodApiError("connection refused".into()))
            }
            async fn start_pod(&self, _pod_id: &str) -> Result<(), PodApiError> {
                Ok(())
            }
            async fn stop_pod(&self, _pod_id: &str) -> Result<(), PodApiError> {
                Ok(())
            }
            async fn create_pod(&self, _config: &PodCreateRequest) -> Result<String, PodApiError> {
                Ok("x".into())
            }
        }

        let api = Arc::new(FailGetPodApi);
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Running;
        }

        // Should not error — get_pod failure is just a warning
        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();
    }

    #[tokio::test]
    async fn test_pod_api_error_display() {
        let err = PodApiError("test error".into());
        assert_eq!(format!("{err}"), "RunPod API error: test error");
    }

    // These tests mutate process-global env vars, so they must run in a
    // single test to avoid races with parallel test threads.
    #[test]
    fn test_build_pod_env() {
        // --- with tailscale ---
        // Use unique prefixed env var names wouldn't help here since the
        // function reads fixed names; instead we run both cases sequentially.
        unsafe {
            std::env::set_var("FLOWSTATE_RUNPOD_POD_SERVER_IP", "100.64.0.1");
            std::env::set_var("FLOWSTATE_RUNPOD_POD_SERVER_URL", "http://100.64.0.1:3710");
            std::env::set_var("FLOWSTATE_RUNPOD_POD_API_KEY", "test-api-key");
            std::env::set_var(
                "FLOWSTATE_RUNPOD_POD_VLLM_MODEL",
                "MiniMaxAI/MiniMax-M1-80k",
            );
            std::env::set_var("FLOWSTATE_RUNPOD_POD_VLLM_MAX_MODEL_LEN", "32000");
            std::env::set_var("FLOWSTATE_RUNPOD_POD_HF_TOKEN", "hf_test123");
        }

        let ts_authkey = Some("tskey-auth-abc123".to_string());
        let env = PodManagerConfig::build_pod_env(&ts_authkey);

        let find = |k: &str| {
            env.iter()
                .find(|(key, _)| key == k)
                .map(|(_, v)| v.as_str())
        };
        assert_eq!(find("TS_AUTHKEY"), Some("tskey-auth-abc123"));
        assert_eq!(find("TS_SERVER_IP"), Some("100.64.0.1"));
        assert_eq!(find("FLOWSTATE_SERVER_URL"), Some("http://100.64.0.1:3710"));
        assert_eq!(find("FLOWSTATE_API_KEY"), Some("test-api-key"));
        assert_eq!(find("VLLM_MODEL"), Some("MiniMaxAI/MiniMax-M1-80k"));
        assert_eq!(find("VLLM_MAX_MODEL_LEN"), Some("32000"));
        assert_eq!(find("HF_TOKEN"), Some("hf_test123"));

        // --- without tailscale ---
        unsafe {
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_SERVER_IP");
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_SERVER_URL");
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_API_KEY");
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_VLLM_MODEL");
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_VLLM_MAX_MODEL_LEN");
            std::env::remove_var("FLOWSTATE_RUNPOD_POD_HF_TOKEN");
        }

        let env = PodManagerConfig::build_pod_env(&None);
        assert!(env.is_empty());
    }

    #[tokio::test]
    async fn test_cost_cap_triggers_drain() {
        let state = test_state().await;
        let mut config = test_config();
        config.max_daily_spend_cents = 100;
        let api = Arc::new(MockRunPodApi::new("RUNNING"));
        let pod_state = Arc::new(TokioMutex::new(PodManagerState::new(Some("pod-1".into()))));
        {
            let mut ps = pod_state.lock().await;
            ps.pod_status = PodStatus::Running;
            ps.daily_cost_cents = 101; // over the cap
        }

        // Register a runner
        {
            let mut runners = state.runners.lock().unwrap();
            runners.insert(
                "runner-1".into(),
                RunnerInfo {
                    runner_id: "runner-1".into(),
                    last_seen: chrono::Utc::now(),
                    backend_name: None,
                    capability: None,
                    capabilities: vec![],
                    poll_interval: None,
                    max_concurrent: None,
                    max_builds: None,
                    active_count: None,
                    active_builds: None,
                    status: RunnerStatus::Active,
                    pending_config: None,
                },
            );
        }

        pod_manager_tick(&state, &config, &pod_state, api.as_ref())
            .await
            .unwrap();

        let ps = pod_state.lock().await;
        assert!(ps.cost_capped);
        assert_eq!(ps.pod_status, PodStatus::Draining);
    }
}
