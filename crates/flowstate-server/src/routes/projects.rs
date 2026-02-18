use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::{get, put},
    Json, Router,
};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_service::TaskService;
use serde_json::{json, Value};

use crate::crypto;

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/projects", get(list_projects).post(create_project))
        .route(
            "/api/projects/{id}",
            get(get_project)
                .put(update_project)
                .delete(delete_project),
        )
        .route("/api/projects/by-slug/{slug}", get(get_project_by_slug))
        .route("/api/projects/{id}/repo-token", put(set_repo_token).get(get_repo_token))
}

/// Strip the encrypted token from project responses, replace with a boolean flag.
fn redact_token(mut project: Project) -> Value {
    let has_token = project.repo_token.is_some();
    project.repo_token = None;
    let mut val = json!(project);
    val["has_repo_token"] = json!(has_token);
    val
}

fn redact_tokens(projects: Vec<Project>) -> Value {
    json!(projects.into_iter().map(redact_token).collect::<Vec<_>>())
}

async fn list_projects(
    State(state): State<AppState>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_projects()
        .await
        .map(|p| Json(redact_tokens(p)))
        .map_err(to_error)
}

async fn get_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .get_project(&id)
        .await
        .map(|p| Json(redact_token(p)))
        .map_err(to_error)
}

async fn get_project_by_slug(
    State(state): State<AppState>,
    Path(slug): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .get_project_by_slug(&slug)
        .await
        .map(|p| Json(redact_token(p)))
        .map_err(to_error)
}

async fn create_project(
    State(state): State<AppState>,
    Json(input): Json<CreateProject>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state
        .service
        .create_project(&input)
        .await
        .map(|p| (StatusCode::CREATED, Json(redact_token(p))))
        .map_err(to_error)
}

async fn update_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateProject>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .update_project(&id, &input)
        .await
        .map(|p| Json(redact_token(p)))
        .map_err(to_error)
}

async fn delete_project(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state
        .service
        .delete_project(&id)
        .await
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(to_error)
}

#[derive(serde::Deserialize)]
struct SetRepoTokenInput {
    token: String,
}

/// PUT /api/projects/{id}/repo-token — encrypt and store.
async fn set_repo_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<SetRepoTokenInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let encrypted = crypto::encrypt(&state.encryption_key, &input.token)
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(e)))?;

    state
        .service
        .update_project(
            &id,
            &UpdateProject {
                repo_token: Some(encrypted),
                ..Default::default()
            },
        )
        .await
        .map_err(to_error)?;

    Ok(Json(json!({ "status": "ok" })))
}

/// GET /api/projects/{id}/repo-token — decrypt and return (for runner use).
async fn get_repo_token(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let project = state.service.get_project(&id).await.map_err(to_error)?;

    match project.repo_token {
        Some(encrypted) => {
            let plaintext = crypto::decrypt(&state.encryption_key, &encrypted).map_err(|e| {
                to_error(flowstate_service::ServiceError::Internal(format!(
                    "decrypt: {e}"
                )))
            })?;
            Ok(Json(json!({ "token": plaintext })))
        }
        None => Err(to_error(flowstate_service::ServiceError::NotFound(
            "no repo token set".into(),
        ))),
    }
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
