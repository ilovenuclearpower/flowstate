use axum::{
    extract::{Path, State},
    http::StatusCode,
    routing::get,
    Json, Router,
};
use flowstate_core::task_pr::CreateTaskPr;
use flowstate_service::TaskService;
use serde::Deserialize;
use serde_json::{json, Value};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new().route(
        "/api/tasks/{task_id}/prs",
        get(list_task_prs).post(create_task_pr),
    )
}

#[derive(Deserialize)]
struct CreateTaskPrRequest {
    pub claude_run_id: Option<String>,
    pub pr_url: String,
    pub pr_number: i64,
    pub branch_name: String,
}

async fn create_task_pr(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(body): Json<CreateTaskPrRequest>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let input = CreateTaskPr {
        task_id,
        claude_run_id: body.claude_run_id,
        pr_url: body.pr_url,
        pr_number: body.pr_number,
        branch_name: body.branch_name,
    };
    state
        .service
        .create_task_pr(&input)
        .await
        .map(|pr| (StatusCode::CREATED, Json(json!(pr))))
        .map_err(to_error)
}

async fn list_task_prs(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_task_prs(&task_id)
        .await
        .map(|prs| Json(json!(prs)))
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
