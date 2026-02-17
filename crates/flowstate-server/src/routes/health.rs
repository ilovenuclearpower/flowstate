use axum::{extract::State, routing::get, Json, Router};
use chrono::Utc;
use serde_json::{json, Value};

use super::AppState;

/// Public routes (no auth required).
pub fn routes() -> Router<AppState> {
    Router::new().route("/api/health", get(health))
}

/// Protected routes (auth required).
pub fn protected_routes() -> Router<AppState> {
    Router::new().route("/api/status", get(system_status))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}

async fn system_status(State(state): State<AppState>) -> Json<Value> {
    let now = Utc::now();
    let stale_threshold = chrono::Duration::minutes(5);
    let connected_threshold = chrono::Duration::seconds(30);

    let mut runners_lock = state.runners.lock().unwrap();

    // Prune runners not seen in 5 minutes
    runners_lock.retain(|_, info| now - info.last_seen < stale_threshold);

    let runners: Vec<Value> = runners_lock
        .values()
        .map(|info| {
            let connected = now - info.last_seen < connected_threshold;
            json!({
                "runner_id": info.runner_id,
                "last_seen": info.last_seen.to_rfc3339(),
                "connected": connected,
            })
        })
        .collect();

    drop(runners_lock);

    Json(json!({
        "server": "ok",
        "runners": runners,
    }))
}
