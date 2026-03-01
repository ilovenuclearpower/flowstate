use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::AppState;
use crate::auth::{build_auth_config_from_db, generate_api_key, sha256_hex};

/// Public (no auth) setup routes.
pub fn setup_routes() -> Router<AppState> {
    Router::new()
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup/init", post(setup_init))
}

/// Protected (auth required) admin routes.
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/api-keys", get(list_api_keys))
        .route("/api/admin/api-keys", post(create_api_key))
        .route("/api/admin/api-keys/{id}", delete(revoke_api_key))
}

#[derive(Serialize)]
struct SetupStatusResponse {
    setup_needed: bool,
}

async fn setup_status(
    State(state): State<AppState>,
) -> Result<Json<SetupStatusResponse>, (StatusCode, Json<Value>)> {
    // If auth is already configured (env key or DB keys), setup is not needed
    let auth_guard = state.auth.read().await;
    if auth_guard.is_some() {
        return Ok(Json(SetupStatusResponse {
            setup_needed: false,
        }));
    }
    drop(auth_guard);

    let has_keys = state.db.has_api_keys().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    Ok(Json(SetupStatusResponse {
        setup_needed: !has_keys,
    }))
}

#[derive(Deserialize)]
struct SetupInitInput {
    name: String,
}

#[derive(Serialize)]
struct SetupInitResponse {
    api_key: String,
    id: String,
    name: String,
}

async fn setup_init(
    State(state): State<AppState>,
    Json(input): Json<SetupInitInput>,
) -> Result<Json<SetupInitResponse>, (StatusCode, Json<Value>)> {
    // Forbid if auth is already configured (env key or existing DB keys)
    let auth_guard = state.auth.read().await;
    if auth_guard.is_some() {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "setup already completed — authentication is configured"})),
        ));
    }
    drop(auth_guard);

    // Forbid if any DB key already exists (race protection)
    let has_keys = state.db.has_api_keys().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    if has_keys {
        return Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "setup already completed — API keys exist"})),
        ));
    }

    // Generate and insert the first key
    let raw_key = generate_api_key();
    let hash = sha256_hex(&raw_key);
    let api_key = state
        .db
        .insert_api_key(&input.name, &hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    // Hot-reload: activate auth immediately
    if let Some(new_auth) = build_auth_config_from_db(state.db.clone()).await {
        let mut auth_write = state.auth.write().await;
        // Only activate if not already enabled (race protection)
        if auth_write.is_none() {
            *auth_write = Some(new_auth);
        }
    }

    Ok(Json(SetupInitResponse {
        api_key: raw_key,
        id: api_key.id,
        name: api_key.name,
    }))
}

#[derive(Serialize)]
struct ApiKeyInfo {
    id: String,
    name: String,
    created_at: String,
    last_used_at: Option<String>,
}

async fn list_api_keys(
    State(state): State<AppState>,
) -> Result<Json<Vec<ApiKeyInfo>>, (StatusCode, Json<Value>)> {
    let keys = state.db.list_api_keys().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let infos: Vec<ApiKeyInfo> = keys
        .into_iter()
        .map(|k| ApiKeyInfo {
            id: k.id,
            name: k.name,
            created_at: k.created_at,
            last_used_at: k.last_used_at,
        })
        .collect();

    Ok(Json(infos))
}

#[derive(Deserialize)]
struct CreateApiKeyInput {
    name: String,
}

#[derive(Serialize)]
struct GenerateKeyResponse {
    api_key: String,
    id: String,
    name: String,
}

async fn create_api_key(
    State(state): State<AppState>,
    Json(input): Json<CreateApiKeyInput>,
) -> Result<Json<GenerateKeyResponse>, (StatusCode, Json<Value>)> {
    let raw_key = generate_api_key();
    let hash = sha256_hex(&raw_key);
    let api_key = state
        .db
        .insert_api_key(&input.name, &hash)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    // If auth was previously None (open access), activate it now
    {
        let auth_read = state.auth.read().await;
        if auth_read.is_none() {
            drop(auth_read);
            if let Some(new_auth) = build_auth_config_from_db(state.db.clone()).await {
                let mut auth_write = state.auth.write().await;
                if auth_write.is_none() {
                    *auth_write = Some(new_auth);
                }
            }
        }
    }

    Ok(Json(GenerateKeyResponse {
        api_key: raw_key,
        id: api_key.id,
        name: api_key.name,
    }))
}

async fn revoke_api_key(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state.db.delete_api_key(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode as AxumStatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    use crate::test_helpers::{test_router, test_router_with_auth};

    #[tokio::test]
    async fn setup_status_true_when_no_keys() {
        let app = test_router().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/setup/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["setup_needed"], true);
    }

    #[tokio::test]
    async fn setup_init_creates_first_key() {
        let app = test_router().await;

        // Init setup
        let body = serde_json::to_string(&serde_json::json!({"name": "admin"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/setup/init")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v["api_key"].as_str().unwrap().starts_with("fs_"));
        assert!(!v["id"].as_str().unwrap().is_empty());
        assert_eq!(v["name"], "admin");

        // Now setup_status should return false
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/setup/status")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v["setup_needed"], false);
    }

    #[tokio::test]
    async fn setup_init_forbidden_after_key_exists() {
        let app = test_router().await;

        // First init succeeds
        let body = serde_json::to_string(&serde_json::json!({"name": "admin"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/setup/init")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);

        // Second init fails with 403
        let body = serde_json::to_string(&serde_json::json!({"name": "second"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/setup/init")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::FORBIDDEN);
    }

    #[tokio::test]
    async fn list_api_keys_empty_then_populated() {
        let (app, api_key) = test_router_with_auth().await;

        // List is initially empty (env key doesn't show up in DB)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/api-keys")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.as_array().unwrap().is_empty());

        // Generate a key via admin API
        let body = serde_json::to_string(&serde_json::json!({"name": "runner-key"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/api-keys")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let gen: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(gen["api_key"].as_str().unwrap().starts_with("fs_"));
        assert_eq!(gen["name"], "runner-key");

        // List should now have 1 key
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/api-keys")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(v.as_array().unwrap().len(), 1);
        assert_eq!(v[0]["name"], "runner-key");
        // key_hash should NOT be present
        assert!(v[0].get("key_hash").is_none());
    }

    #[tokio::test]
    async fn revoke_api_key_removes_key() {
        let (app, api_key) = test_router_with_auth().await;

        // Generate a key
        let body = serde_json::to_string(&serde_json::json!({"name": "to-revoke"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/api-keys")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let gen: Value = serde_json::from_slice(&bytes).unwrap();
        let key_id = gen["id"].as_str().unwrap();

        // Revoke
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(&format!("/api/admin/api-keys/{key_id}"))
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::NO_CONTENT);

        // List should be empty
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/api-keys")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        assert!(v.as_array().unwrap().is_empty());
    }

    #[tokio::test]
    async fn admin_endpoints_require_auth() {
        let (app, _api_key) = test_router_with_auth().await;

        // list without auth
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri("/api/admin/api-keys")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::UNAUTHORIZED);

        // create without auth
        let body = serde_json::to_string(&serde_json::json!({"name": "test"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/admin/api-keys")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::UNAUTHORIZED);

        // delete without auth
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri("/api/admin/api-keys/some-id")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::UNAUTHORIZED);
    }
}
