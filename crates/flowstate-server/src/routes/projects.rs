use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use flowstate_core::project::{CreateProject, UpdateProject};
use flowstate_service::TaskService;
use serde_json::{json, Value};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route("/api/projects/{id}", get(get_project).put(update_project).delete(delete_project))
        .route("/api/projects/by-slug/{slug}", get(get_project_by_slug))
}

async fn list_projects(State(state): State<AppState>) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.list_projects().await
        .map(|p| Json(json!(p)))
        .map_err(to_error)
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.get_project(&id).await
        .map(|p| Json(json!(p)))
        .map_err(to_error)
}

async fn get_project_by_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.get_project_by_slug(&slug).await
        .map(|p| Json(json!(p)))
        .map_err(to_error)
}

async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<CreateProject>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.service.create_project(&input).await
        .map(|p| (StatusCode::CREATED, Json(json!(p))))
        .map_err(to_error)
}

async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateProject>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.update_project(&id, &input).await
        .map(|p| Json(json!(p)))
        .map_err(to_error)
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state.service.delete_project(&id).await
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
