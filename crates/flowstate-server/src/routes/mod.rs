pub mod claude_runs;
pub mod health;
pub mod projects;
pub mod task_links;
pub mod task_prs;
pub mod tasks;

use std::collections::HashMap;
use std::sync::Arc;

use aes_gcm::{Aes256Gcm, Key};
use axum::{middleware, Router};
use chrono::{DateTime, Utc};
use flowstate_db::Db;
use flowstate_service::LocalService;
use flowstate_store::ObjectStore;

use crate::auth::{auth_middleware, AuthConfig};

pub struct RunnerInfo {
    pub runner_id: String,
    pub last_seen: DateTime<Utc>,
}

pub struct InnerAppState {
    pub service: LocalService,
    pub db: Db,
    pub auth: Option<Arc<AuthConfig>>,
    pub runners: std::sync::Mutex<HashMap<String, RunnerInfo>>,
    pub encryption_key: Key<Aes256Gcm>,
    pub store: Arc<dyn ObjectStore>,
}

pub type AppState = Arc<InnerAppState>;

pub fn build_router(
    service: LocalService,
    db: Db,
    auth: Option<Arc<AuthConfig>>,
    encryption_key: Key<Aes256Gcm>,
    store: Arc<dyn ObjectStore>,
) -> Router {
    let state: AppState = Arc::new(InnerAppState {
        service,
        db,
        auth,
        runners: std::sync::Mutex::new(HashMap::new()),
        encryption_key,
        store,
    });

    let public = Router::new().merge(health::routes());

    let protected = Router::new()
        .merge(projects::routes())
        .merge(tasks::routes())
        .merge(task_links::routes())
        .merge(task_prs::routes())
        .merge(claude_runs::routes())
        .merge(health::protected_routes())
        .route_layer(middleware::from_fn_with_state(
            state.clone(),
            auth_middleware,
        ));

    public.merge(protected).with_state(state)
}
