pub mod auth;
pub mod crypto;
mod routes;
pub mod watchdog;

use std::sync::Arc;

use anyhow::Result;
use flowstate_db::Db;
use flowstate_service::LocalService;
use tokio::net::TcpListener;

use auth::AuthConfig;

pub async fn serve(listener: TcpListener, db: Db, auth: Option<Arc<AuthConfig>>) -> Result<()> {
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
    let app = routes::build_router(service, db.clone(), auth, encryption_key, store);

    // Launch the watchdog background task (scans every 60 seconds)
    let watchdog_db = db;
    tokio::spawn(async move {
        watchdog::run_watchdog(watchdog_db, 60).await;
    });

    axum::serve(listener, app).await?;
    Ok(())
}
