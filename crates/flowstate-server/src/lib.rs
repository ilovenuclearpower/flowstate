pub mod auth;
pub mod crypto;
pub mod pod_manager;
#[cfg(any(test, feature = "test-helpers"))]
pub mod routes;
#[cfg(not(any(test, feature = "test-helpers")))]
mod routes;
pub mod watchdog;

#[cfg(any(test, feature = "test-helpers"))]
pub mod test_helpers;

use std::collections::HashMap;
use std::sync::Arc;

use anyhow::Result;
use flowstate_db::Database;
use flowstate_service::LocalService;
use tokio::net::TcpListener;

use auth::AuthConfig;
use routes::{AppState, InnerAppState};

pub async fn serve(
    listener: TcpListener,
    db: Arc<dyn Database>,
    auth: Option<Arc<AuthConfig>>,
) -> Result<()> {
    let encryption_key = crypto::load_or_generate_key();
    let store_config = flowstate_store::StoreConfig::from_env();
    if store_config.is_s3() {
        tracing::info!(
            "storage backend: s3 (endpoint={}, bucket={})",
            store_config.endpoint_url.as_deref().unwrap_or("?"),
            store_config.bucket.as_deref().unwrap_or("?"),
        );
    } else {
        tracing::info!(
            "storage backend: local ({})",
            store_config
                .local_data_dir
                .as_deref()
                .unwrap_or(&flowstate_db::data_dir().to_string_lossy()),
        );
    }
    let store = flowstate_store::create_store(&store_config)
        .map_err(|e| anyhow::anyhow!("failed to create object store: {e}"))?;
    let service = LocalService::new(db.clone());

    // Check for pod manager configuration
    let pod_manager_state = pod_manager::PodManagerConfig::from_env().map(|pm_config| {
        let pod_id = pm_config.pod_id.clone();
        let state = Arc::new(tokio::sync::Mutex::new(pod_manager::PodManagerState::new(
            pod_id,
        )));
        (pm_config, state)
    });

    let state: AppState = Arc::new(InnerAppState {
        service,
        db: db.clone(),
        auth,
        runners: std::sync::Mutex::new(HashMap::new()),
        encryption_key,
        store,
        pod_manager: pod_manager_state.as_ref().map(|(_, s)| s.clone()),
    });

    let app = routes::build_router(state.clone());

    // Launch the watchdog background task (scans every 60 seconds)
    let watchdog_db = db;
    tokio::spawn(async move {
        watchdog::run_watchdog(watchdog_db, 60).await;
    });

    // Launch the pod manager background task if configured
    if let Some((pm_config, pm_state)) = pod_manager_state {
        let pm_app_state = state;
        let api = Arc::new(pod_manager::RunPodClient::new(&pm_config.api_key));
        tracing::info!("pod manager enabled (pod_id={:?})", pm_config.pod_id);
        tokio::spawn(async move {
            pod_manager::run_pod_manager(pm_app_state, pm_config, pm_state, api).await;
        });
    }

    axum::serve(listener, app).await?;
    Ok(())
}
