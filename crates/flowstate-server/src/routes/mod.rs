pub mod claude_runs;
pub mod health;
pub mod infra;
pub mod projects;
pub mod sprints;
pub mod task_links;
pub mod task_prs;
pub mod tasks;

use std::collections::HashMap;
use std::sync::Arc;

use aes_gcm::{Aes256Gcm, Key};
use axum::{middleware, Router};
use chrono::{DateTime, Utc};
use flowstate_db::Database;
use flowstate_service::LocalService;
use flowstate_store::ObjectStore;
use serde::{Deserialize, Serialize};

use crate::auth::{auth_middleware, AuthConfig};
use crate::pod_manager::PodManagerState;

/// Pending configuration changes to be delivered to a runner via registration response.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PendingConfig {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub poll_interval: Option<u64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub drain: Option<bool>,
}

/// Runner lifecycle status.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RunnerStatus {
    Active,
    Draining,
    Drained,
}

pub struct RunnerInfo {
    pub runner_id: String,
    pub last_seen: DateTime<Utc>,
    pub backend_name: Option<String>,
    pub capability: Option<String>,
    /// The set of capability tiers this runner can handle (e.g., ["light", "standard", "heavy"]).
    pub capabilities: Vec<String>,
    /// Runner's current poll interval in seconds.
    pub poll_interval: Option<u64>,
    /// Maximum concurrent runs the runner supports.
    pub max_concurrent: Option<usize>,
    /// Maximum concurrent build actions the runner supports.
    pub max_builds: Option<usize>,
    /// Number of currently active runs.
    pub active_count: Option<usize>,
    /// Number of currently active build runs.
    pub active_builds: Option<usize>,
    /// Runner lifecycle status.
    pub status: RunnerStatus,
    /// Pending configuration to deliver on next registration heartbeat.
    pub pending_config: Option<PendingConfig>,
}

pub struct InnerAppState {
    pub service: LocalService,
    pub db: Arc<dyn Database>,
    pub auth: Option<Arc<AuthConfig>>,
    pub runners: std::sync::Mutex<HashMap<String, RunnerInfo>>,
    pub encryption_key: Key<Aes256Gcm>,
    pub store: Arc<dyn ObjectStore>,
    pub pod_manager: Option<Arc<tokio::sync::Mutex<PodManagerState>>>,
}

pub type AppState = Arc<InnerAppState>;

pub fn build_router(state: AppState) -> Router {
    let public = Router::new().merge(health::routes());

    let protected = Router::new()
        .merge(projects::routes())
        .merge(tasks::routes())
        .merge(sprints::routes())
        .merge(task_links::routes())
        .merge(task_prs::routes())
        .merge(claude_runs::routes())
        .merge(infra::routes())
        .merge(health::protected_routes())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    public.merge(protected).with_state(state)
}
