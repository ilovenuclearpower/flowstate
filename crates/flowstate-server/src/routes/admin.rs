use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Extension, Json, Router,
};
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use super::AppState;
use crate::auth::{build_auth_config_from_db, generate_api_key, sha256_hex, ApiKeyRole};

/// Public (no auth) setup routes.
pub fn setup_routes() -> Router<AppState> {
    Router::new()
        .route("/api/setup/status", get(setup_status))
        .route("/api/setup/init", post(setup_init))
        .route("/api/runners/handshake", post(runner_handshake))
}

/// Protected (auth required) admin routes.
pub fn protected_routes() -> Router<AppState> {
    Router::new()
        .route("/api/admin/api-keys", get(list_api_keys))
        .route("/api/admin/api-keys", post(create_api_key))
        .route("/api/admin/api-keys/{id}", delete(revoke_api_key))
        .route("/api/admin/handshakes", get(list_handshakes))
        .route(
            "/api/admin/handshakes/{id}/approve",
            post(approve_handshake),
        )
        .route("/api/admin/handshakes/{id}/reject", post(reject_handshake))
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
        .insert_api_key(&input.name, &hash, "admin")
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
    role: Option<Extension<ApiKeyRole>>,
    Json(input): Json<CreateApiKeyInput>,
) -> Result<Json<GenerateKeyResponse>, (StatusCode, Json<Value>)> {
    require_admin_role(&role)?;

    let raw_key = generate_api_key();
    let hash = sha256_hex(&raw_key);
    let api_key = state
        .db
        .insert_api_key(&input.name, &hash, "admin")
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
    role: Option<Extension<ApiKeyRole>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    require_admin_role(&role)?;

    state.db.delete_api_key(&id).await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    Ok(StatusCode::NO_CONTENT)
}

/// Reject the request if the caller's API key role is not "admin".
fn require_admin_role(
    role: &Option<Extension<ApiKeyRole>>,
) -> Result<(), (StatusCode, Json<Value>)> {
    match role {
        Some(Extension(r)) if r.0 == "admin" => Ok(()),
        Some(_) => Err((
            StatusCode::FORBIDDEN,
            Json(json!({"error": "admin role required"})),
        )),
        // No role extension means auth wasn't configured or this is open access
        None => Ok(()),
    }
}

// -- Runner Handshake (public, unauthenticated) --

#[derive(Deserialize)]
struct HandshakeInput {
    runner_id: String,
    #[serde(default)]
    hostname: String,
}

#[derive(Serialize)]
struct HandshakeResponse {
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_key: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    handshake_id: Option<String>,
}

async fn runner_handshake(
    State(state): State<AppState>,
    Json(input): Json<HandshakeInput>,
) -> Result<Json<HandshakeResponse>, (StatusCode, Json<Value>)> {
    let handshake = state
        .db
        .upsert_runner_handshake(&input.runner_id, &input.hostname)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    match handshake.status.as_str() {
        "approved" => {
            // Return the raw key and transition to claimed
            let raw_key = state
                .db
                .claim_runner_handshake(&handshake.id)
                .await
                .map_err(|e| {
                    (
                        StatusCode::INTERNAL_SERVER_ERROR,
                        Json(json!({"error": e.to_string()})),
                    )
                })?;
            Ok(Json(HandshakeResponse {
                status: "approved".to_string(),
                api_key: raw_key,
                handshake_id: Some(handshake.id),
            }))
        }
        _ => Ok(Json(HandshakeResponse {
            status: handshake.status,
            api_key: None,
            handshake_id: Some(handshake.id),
        })),
    }
}

// -- Admin Handshake Management (protected) --

#[derive(Serialize)]
struct HandshakeInfo {
    id: String,
    runner_id: String,
    hostname: String,
    status: String,
    created_at: String,
    updated_at: String,
}

async fn list_handshakes(
    State(state): State<AppState>,
    role: Option<Extension<ApiKeyRole>>,
) -> Result<Json<Vec<HandshakeInfo>>, (StatusCode, Json<Value>)> {
    require_admin_role(&role)?;

    let handshakes = state.db.list_runner_handshakes().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;

    let infos: Vec<HandshakeInfo> = handshakes
        .into_iter()
        .map(|h| HandshakeInfo {
            id: h.id,
            runner_id: h.runner_id,
            hostname: h.hostname,
            status: h.status,
            created_at: h.created_at,
            updated_at: h.updated_at,
        })
        .collect();

    Ok(Json(infos))
}

#[derive(Serialize)]
struct ApproveHandshakeResponse {
    api_key_name: String,
}

async fn approve_handshake(
    State(state): State<AppState>,
    role: Option<Extension<ApiKeyRole>>,
    Path(id): Path<String>,
) -> Result<Json<ApproveHandshakeResponse>, (StatusCode, Json<Value>)> {
    require_admin_role(&role)?;

    // Generate an API key with role=runner
    let raw_key = generate_api_key();
    let hash = sha256_hex(&raw_key);

    // Look up the handshake to get the runner_id for the key name
    let handshakes = state.db.list_runner_handshakes().await.map_err(|e| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({"error": e.to_string()})),
        )
    })?;
    let handshake = handshakes.iter().find(|h| h.id == id).ok_or_else(|| {
        (
            StatusCode::NOT_FOUND,
            Json(json!({"error": "handshake not found"})),
        )
    })?;

    let key_name = format!("runner-{}", handshake.runner_id);
    let api_key = state
        .db
        .insert_api_key(&key_name, &hash, "runner")
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    // Store the raw key in the handshake record for the runner to claim
    state
        .db
        .approve_runner_handshake(&id, &api_key.id, &raw_key)
        .await
        .map_err(|e| {
            (
                StatusCode::INTERNAL_SERVER_ERROR,
                Json(json!({"error": e.to_string()})),
            )
        })?;

    // Hot-reload auth if needed (a new DB key was created)
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

    Ok(Json(ApproveHandshakeResponse {
        api_key_name: key_name,
    }))
}

async fn reject_handshake(
    State(state): State<AppState>,
    role: Option<Extension<ApiKeyRole>>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    require_admin_role(&role)?;

    state.db.reject_runner_handshake(&id).await.map_err(|e| {
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
