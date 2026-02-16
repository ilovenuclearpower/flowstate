use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{delete, get, post},
    Json, Router,
};
use flowstate_core::task_link::CreateTaskLink;
use flowstate_service::TaskService;
use serde_json::{json, Value};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/task-links", post(create_task_link))
        .route("/api/task-links/{id}", delete(delete_task_link))
        .route("/api/tasks/{task_id}/links", get(list_task_links))
}

async fn create_task_link(
    State(state): State<AppState>,
    Json(input): Json<CreateTaskLink>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.service.create_task_link(&input)
        .map(|l| (StatusCode::CREATED, Json(json!(l))))
        .map_err(to_error)
}

async fn list_task_links(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.list_task_links(&task_id)
        .map(|l| Json(json!(l)))
        .map_err(to_error)
}

async fn delete_task_link(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state.service.delete_task_link(&id)
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
