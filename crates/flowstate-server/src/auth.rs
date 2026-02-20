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
    let env_key_hash = std::env::var("FLOWSTATE_API_KEY")
        .ok()
        .filter(|k| !k.is_empty())
        .map(|k| sha256_hex(&k));

    let has_db_keys = db.has_api_keys().await.unwrap_or(false);

    if env_key_hash.is_none() && !has_db_keys {
        return None;
    }

    Some(Arc::new(AuthConfig {
        env_key_hash,
        db,
    }))
}
