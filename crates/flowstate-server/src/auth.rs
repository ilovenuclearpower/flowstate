use std::sync::Arc;

use axum::{
    extract::{Request, State},
    http::StatusCode,
    middleware::Next,
    response::{IntoResponse, Response},
    Json,
};
use sha2::{Digest, Sha256};
use serde_json::json;

use flowstate_db::Database;

use crate::routes::AppState;

/// Authentication configuration.
pub struct AuthConfig {
    /// SHA-256 hash of the `FLOWSTATE_API_KEY` env var (if set).
    pub env_key_hash: Option<String>,
    /// Database handle for DB-backed API keys.
    pub db: Arc<dyn Database>,
}

/// SHA-256 hash a raw key, returning the hex-encoded digest.
pub fn sha256_hex(input: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(input.as_bytes());
    format!("{:x}", hasher.finalize())
}

/// Generate a new API key: `fs_` + 43 chars of base62-encoded random bytes.
pub fn generate_api_key() -> String {
    use rand::Rng;
    const BASE62: &[u8] = b"0123456789ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz";
    let mut rng = rand::thread_rng();
    let random_part: String = (0..43)
        .map(|_| {
            let idx = rng.gen_range(0..BASE62.len());
            BASE62[idx] as char
        })
        .collect();
    format!("fs_{random_part}")
}

/// Axum middleware that enforces authentication.
///
/// If `auth` is `None` in the AppState, all requests pass through (open access).
/// Otherwise, requires a valid `Authorization: Bearer <token>` header.
pub async fn auth_middleware(
    State(state): State<AppState>,
    request: Request,
    next: Next,
) -> Response {
    let auth = match &state.auth {
        Some(auth) => auth,
        None => return next.run(request).await,
    };

    // Extract bearer token
    let token = request
        .headers()
        .get("authorization")
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.strip_prefix("Bearer "));

    let token = match token {
        Some(t) => t,
        None => {
            return (
                StatusCode::UNAUTHORIZED,
                Json(json!({ "error": "missing or invalid API key" })),
            )
                .into_response();
        }
    };

    let token_hash = sha256_hex(token);

    // Check env key (constant-time comparison via hash equality)
    if let Some(ref env_hash) = auth.env_key_hash {
        if constant_time_eq(&token_hash, env_hash) {
            return next.run(request).await;
        }
    }

    // Check DB keys
    let db = auth.db.clone();
    let hash_for_db = token_hash.clone();
    match db.find_api_key_by_hash(&hash_for_db).await {
        Ok(Some(api_key)) => {
            // Fire-and-forget: update last_used_at
            let db2 = db.clone();
            let key_id = api_key.id.clone();
            tokio::spawn(async move {
                let _ = db2.touch_api_key(&key_id).await;
            });
            return next.run(request).await;
        }
        Ok(None) => {}
        Err(_) => {}
    }

    (
        StatusCode::UNAUTHORIZED,
        Json(json!({ "error": "missing or invalid API key" })),
    )
        .into_response()
}

/// Constant-time string comparison to prevent timing attacks.
fn constant_time_eq(a: &str, b: &str) -> bool {
    if a.len() != b.len() {
        return false;
    }
    a.as_bytes()
        .iter()
        .zip(b.as_bytes())
        .fold(0u8, |acc, (x, y)| acc | (x ^ y))
        == 0
}

/// Build an `Option<AuthConfig>` from env + DB state.
///
/// Returns `None` (open access) when neither `FLOWSTATE_API_KEY` is set
/// nor any DB-backed keys exist.
pub async fn build_auth_config(db: Arc<dyn Database>) -> Option<Arc<AuthConfig>> {
    let env_key = std::env::var("FLOWSTATE_API_KEY").ok();
    build_auth_config_with_key(db, env_key.as_deref()).await
}

/// Build auth config from an explicit key value (testable without env mutation).
pub async fn build_auth_config_with_key(
    db: Arc<dyn Database>,
    env_key: Option<&str>,
) -> Option<Arc<AuthConfig>> {
    let env_key_hash = env_key
        .filter(|k| !k.is_empty())
        .map(sha256_hex);

    let has_db_keys = db.has_api_keys().await.unwrap_or(false);

    if env_key_hash.is_none() && !has_db_keys {
        return None;
    }

    Some(Arc::new(AuthConfig {
        env_key_hash,
        db,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_sha256_hex_deterministic() {
        let a = sha256_hex("test-input");
        let b = sha256_hex("test-input");
        assert_eq!(a, b);
    }

    #[test]
    fn test_sha256_hex_known_vector() {
        // SHA-256("hello") is well-known
        assert_eq!(
            sha256_hex("hello"),
            "2cf24dba5fb0a30e26e83b2ac5b9e29e1b161e5c1fa7425e73043362938b9824"
        );
    }

    #[test]
    fn test_sha256_hex_empty_string() {
        assert_eq!(
            sha256_hex(""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
    }

    #[test]
    fn test_generate_api_key_format() {
        let key = generate_api_key();
        assert!(key.starts_with("fs_"), "key should start with 'fs_': {key}");
        assert_eq!(key.len(), 46, "key should be 46 chars: {key}");
        // Remaining 43 chars should be alphanumeric (base62)
        assert!(
            key[3..].chars().all(|c| c.is_ascii_alphanumeric()),
            "key suffix should be base62: {key}"
        );
    }

    #[test]
    fn test_generate_api_key_uniqueness() {
        let a = generate_api_key();
        let b = generate_api_key();
        assert_ne!(a, b, "two generated keys should differ");
    }

    #[test]
    fn test_constant_time_eq_equal() {
        assert!(constant_time_eq("hello", "hello"));
    }

    #[test]
    fn test_constant_time_eq_unequal() {
        assert!(!constant_time_eq("hello", "world"));
    }

    #[test]
    fn test_constant_time_eq_different_length() {
        assert!(!constant_time_eq("short", "longer-string"));
    }

    #[test]
    fn test_constant_time_eq_empty() {
        assert!(constant_time_eq("", ""));
    }

    #[tokio::test]
    async fn build_auth_config_no_keys() {
        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
        let config = build_auth_config_with_key(db, None).await;
        // No env key, no DB keys → open access
        assert!(config.is_none());
    }

    #[tokio::test]
    async fn build_auth_config_env_key() {
        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
        let config = build_auth_config_with_key(db, Some("test-key-for-auth")).await;
        assert!(config.is_some());
        let auth = config.unwrap();
        assert!(auth.env_key_hash.is_some());
    }

    #[tokio::test]
    async fn auth_middleware_no_config_passes_all() {
        use crate::test_helpers::test_router;
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let app = test_router().await;
        // Protected endpoint should work without auth header since no auth configured
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_middleware_valid_bearer() {
        use crate::test_helpers::test_router_with_auth;
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let (app, api_key) = test_router_with_auth().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .header("Authorization", format!("Bearer {api_key}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }

    #[tokio::test]
    async fn auth_middleware_invalid_bearer() {
        use crate::test_helpers::test_router_with_auth;
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let (app, _api_key) = test_router_with_auth().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .header("Authorization", "Bearer wrong-key")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_middleware_missing_header() {
        use crate::test_helpers::test_router_with_auth;
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let (app, _api_key) = test_router_with_auth().await;
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/projects")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::UNAUTHORIZED);
    }

    #[tokio::test]
    async fn auth_health_endpoint_no_auth_required() {
        use crate::test_helpers::test_router_with_auth;
        use axum::body::Body;
        use axum::http::Request;
        use tower::ServiceExt;

        let (app, _api_key) = test_router_with_auth().await;
        // Health is a public endpoint — should work without auth
        let resp = app
            .oneshot(
                Request::builder()
                    .uri("/api/health")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
    }
}
