use axum::{routing::get, Json, Router};
use serde_json::{json, Value};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route("/api/health", get(health))
}

async fn health() -> Json<Value> {
    Json(json!({ "status": "ok" }))
}
