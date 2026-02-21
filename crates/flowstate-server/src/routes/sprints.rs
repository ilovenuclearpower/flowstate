use axum::{
    extract::{Path, Query, State},
    http::StatusCode,
    routing::{delete, get, post, put},
    Json, Router,
};
use flowstate_core::sprint::{CreateSprint, UpdateSprint};
use flowstate_service::TaskService;
use serde::Deserialize;
use serde_json::{json, Value};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/sprints", post(create_sprint))
        .route("/api/sprints", get(list_sprints))
        .route("/api/sprints/{id}", get(get_sprint))
        .route("/api/sprints/{id}", put(update_sprint))
        .route("/api/sprints/{id}", delete(delete_sprint))
}

#[derive(Deserialize)]
struct ListSprintsQuery {
    project_id: String,
}

async fn create_sprint(
    State(state): State<AppState>,
    Json(input): Json<CreateSprint>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state
        .service
        .create_sprint(&input)
        .await
        .map(|s| (StatusCode::CREATED, Json(json!(s))))
        .map_err(to_error)
}

async fn get_sprint(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .get_sprint(&id)
        .await
        .map(|s| Json(json!(s)))
        .map_err(to_error)
}

async fn list_sprints(
    State(state): State<AppState>,
    Query(q): Query<ListSprintsQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_sprints(&q.project_id)
        .await
        .map(|s| Json(json!(s)))
        .map_err(to_error)
}

async fn update_sprint(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(update): Json<UpdateSprint>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .update_sprint(&id, &update)
        .await
        .map(|s| Json(json!(s)))
        .map_err(to_error)
}

async fn delete_sprint(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state
        .service
        .delete_sprint(&id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(to_error)
}

fn to_error(e: flowstate_service::ServiceError) -> (StatusCode, Json<Value>) {
    let (status, msg) = match &e {
        flowstate_service::ServiceError::NotFound(_) => (StatusCode::NOT_FOUND, e.to_string()),
        flowstate_service::ServiceError::InvalidInput(_) => {
            (StatusCode::BAD_REQUEST, e.to_string())
        }
        flowstate_service::ServiceError::Internal(_) => {
            (StatusCode::INTERNAL_SERVER_ERROR, e.to_string())
        }
    };
    (status, Json(json!({ "error": msg })))
}
