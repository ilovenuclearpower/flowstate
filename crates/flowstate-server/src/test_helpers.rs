use std::collections::HashMap;
use std::sync::Arc;

use aes_gcm::aead::OsRng;
use aes_gcm::{Aes256Gcm, KeyInit};
use axum::Router;
use flowstate_service::LocalService;
use flowstate_store::StoreConfig;
use tokio::net::TcpListener;

use crate::auth::AuthConfig;
use crate::routes::InnerAppState;

/// Build a test router with in-memory SQLite, temp local store, random AES key, no auth.
pub async fn test_router() -> Router {
    let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
    let service = LocalService::new(db.clone());
    let store_config = StoreConfig {
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
    let key = Aes256Gcm::generate_key(OsRng);
    let state = Arc::new(InnerAppState {
        service,
        db,
        auth: None,
        runners: std::sync::Mutex::new(HashMap::new()),
        encryption_key: key,
        store,
        pod_manager: None,
    });
    crate::routes::build_router(state)
}

/// Build a test router with auth enabled, returning (router, api_key).
pub async fn test_router_with_auth() -> (Router, String) {
    let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
    let service = LocalService::new(db.clone());
    let store_config = StoreConfig {
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
    let key = Aes256Gcm::generate_key(OsRng);
    let api_key = crate::auth::generate_api_key();
    let auth = Arc::new(AuthConfig {
        env_key_hash: Some(crate::auth::sha256_hex(&api_key)),
        db: db.clone(),
    });
    let state = Arc::new(InnerAppState {
        service,
        db,
        auth: Some(auth),
        runners: std::sync::Mutex::new(HashMap::new()),
        encryption_key: key,
        store,
        pod_manager: None,
    });
    let router = crate::routes::build_router(state);
    (router, api_key)
}

/// Build a test router with pod_manager enabled (for infra route tests).
pub async fn test_router_with_pod_manager() -> Router {
    let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
    let service = LocalService::new(db.clone());
    let store_config = StoreConfig {
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
    let key = Aes256Gcm::generate_key(OsRng);
    let pod_state = crate::pod_manager::PodManagerState::new(Some("test-pod-1".into()));
    let state = Arc::new(InnerAppState {
        service,
        db,
        auth: None,
        runners: std::sync::Mutex::new(HashMap::new()),
        encryption_key: key,
        store,
        pod_manager: Some(Arc::new(tokio::sync::Mutex::new(pod_state))),
    });
    crate::routes::build_router(state)
}

/// A running test server with base_url and background task handle.
pub struct TestServer {
    pub base_url: String,
    _handle: tokio::task::JoinHandle<()>,
}

/// Spawn an axum test server on a random port. Returns the TestServer
/// with the `base_url` (e.g. "http://127.0.0.1:12345").
pub async fn spawn_test_server() -> TestServer {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let base_url = format!("http://{addr}");
    let app = test_router().await;
    let handle = tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });
    TestServer {
        base_url,
        _handle: handle,
    }
}
