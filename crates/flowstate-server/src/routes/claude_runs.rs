use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use flowstate_core::claude_run::{ClaudeAction, CreateClaudeRun};
use flowstate_service::TaskService;
use serde::Deserialize;
use serde_json::{json, Value};

use super::AppState;
use crate::claude;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/tasks/{task_id}/claude-runs",
            get(list_claude_runs).post(trigger_claude_run),
        )
        .route("/api/claude-runs/{id}", get(get_claude_run))
        .route("/api/claude-runs/{id}/output", get(get_claude_run_output))
}

#[derive(Debug, Deserialize)]
struct TriggerInput {
    action: String,
}

async fn trigger_claude_run(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(input): Json<TriggerInput>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let action = ClaudeAction::from_str(&input.action).ok_or_else(|| {
        to_error(flowstate_service::ServiceError::InvalidInput(format!(
            "invalid action: {} (expected design, plan, or build)",
            input.action
        )))
    })?;

    let task = state.service.get_task(&task_id).map_err(to_error)?;

    let create = CreateClaudeRun {
        task_id: task_id.clone(),
        action,
    };
    let run = state.service.create_claude_run(&create).map_err(to_error)?;

    // Look up the project for context
    let project = state.service.get_project(&task.project_id).map_err(to_error)?;

    // Spawn async runner
    let db = state.db.clone();
    let run_id = run.id.clone();
    tokio::spawn(async move {
        claude::execute_run(db, run_id, task, project, action).await;
    });

    Ok((StatusCode::CREATED, Json(json!(run))))
}

async fn list_claude_runs(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.list_claude_runs(&task_id)
        .map(|r| Json(json!(r)))
        .map_err(to_error)
}

async fn get_claude_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.get_claude_run(&id)
        .map(|r| Json(json!(r)))
        .map_err(to_error)
}

async fn get_claude_run_output(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<String, (StatusCode, Json<Value>)> {
    // First verify the run exists
    let _run = state.service.get_claude_run(&id).map_err(to_error)?;

    let output_path = flowstate_db::claude_run_dir(&id).join("output.txt");
    std::fs::read_to_string(&output_path).map_err(|e| {
        if e.kind() == std::io::ErrorKind::NotFound {
            to_error(flowstate_service::ServiceError::NotFound(
                "output not yet available".into(),
            ))
        } else {
            to_error(flowstate_service::ServiceError::Internal(format!(
                "read output: {e}"
            )))
        }
    })
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
