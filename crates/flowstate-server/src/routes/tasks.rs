use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    routing::get,
    Json, Router,
};
use flowstate_core::task::{ApprovalStatus, CreateTask, Priority, Status, TaskFilter, UpdateTask};
use flowstate_service::TaskService;
use serde::Deserialize;
use serde_json::{json, Value};
use sha2::{Digest, Sha256};

use super::AppState;

pub fn routes() -> Router<AppState> {
    Router::new()
        .route("/api/tasks", get(list_tasks).post(create_task))
        .route(
            "/api/tasks/{id}",
            get(get_task).put(update_task).delete(delete_task),
        )
        .route("/api/tasks/count-by-status", get(count_by_status))
        .route("/api/tasks/{id}/children", get(list_children))
        .route("/api/tasks/{id}/spec", get(read_spec).put(write_spec))
        .route("/api/tasks/{id}/plan", get(read_plan).put(write_plan))
        .route("/api/tasks/{id}/attachments", get(list_attachments))
}

#[derive(Debug, Deserialize)]
struct TaskQuery {
    project_id: Option<String>,
    status: Option<String>,
    priority: Option<String>,
    sprint_id: Option<String>,
    limit: Option<i64>,
}

async fn list_tasks(
    State(state): State<AppState>,
    Query(q): Query<TaskQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let filter = TaskFilter {
        project_id: q.project_id,
        status: q.status.and_then(|s| Status::from_str(&s)),
        priority: q.priority.and_then(|p| Priority::from_str(&p)),
        sprint_id: q.sprint_id,
        parent_id: None,
        limit: q.limit,
    };
    state.service.list_tasks(&filter)
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.get_task(&id)
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<CreateTask>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state.service.create_task(&input)
        .map(|t| (StatusCode::CREATED, Json(json!(t))))
        .map_err(to_error)
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut input): Json<UpdateTask>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // On spec approval, compute and store the spec content hash
    if input.spec_status == Some(ApprovalStatus::Approved) {
        let spec_path = flowstate_db::task_spec_path(&id);
        if let Ok(content) = std::fs::read_to_string(&spec_path) {
            input.spec_approved_hash = Some(sha256_hex(&content));
        }
    }
    state.service.update_task(&id, &input)
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state.service.delete_task(&id)
        .map(|_| StatusCode::NO_CONTENT)
        .map_err(to_error)
}

#[derive(Debug, Deserialize)]
struct CountQuery {
    project_id: String,
}

async fn count_by_status(
    State(state): State<AppState>,
    Query(q): Query<CountQuery>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.count_tasks_by_status(&q.project_id)
        .map(|c| Json(json!(c)))
        .map_err(to_error)
}

async fn list_children(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.list_child_tasks(&id)
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn read_spec(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // Verify task exists
    let _task = state.service.get_task(&id).map_err(to_error)?;
    let path = flowstate_db::task_spec_path(&id);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(content))
            .unwrap()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(""))
            .unwrap()),
        Err(e) => Err(to_error(flowstate_service::ServiceError::Internal(
            format!("read spec: {e}"),
        ))),
    }
}

async fn write_spec(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let task = state.service.get_task(&id).map_err(to_error)?;
    let path = flowstate_db::task_spec_path(&id);
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(format!("mkdir: {e}"))))?;
    std::fs::write(&path, &body)
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(format!("write: {e}"))))?;

    // Server-side status management
    if task.spec_status == ApprovalStatus::Approved && !task.spec_approved_hash.is_empty() {
        // Revoke approval if content changed
        let new_hash = sha256_hex(&body);
        if new_hash != task.spec_approved_hash {
            let update = UpdateTask {
                spec_status: Some(ApprovalStatus::Pending),
                spec_approved_hash: Some(String::new()),
                ..Default::default()
            };
            let _ = state.service.update_task(&id, &update);
        }
    } else if task.spec_status == ApprovalStatus::None && !body.trim().is_empty() {
        // Auto-set to Pending when content written for the first time
        let update = UpdateTask {
            spec_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update);
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn read_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let _task = state.service.get_task(&id).map_err(to_error)?;
    let path = flowstate_db::task_plan_path(&id);
    match std::fs::read_to_string(&path) {
        Ok(content) => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(content))
            .unwrap()),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(""))
            .unwrap()),
        Err(e) => Err(to_error(flowstate_service::ServiceError::Internal(
            format!("read plan: {e}"),
        ))),
    }
}

async fn write_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let task = state.service.get_task(&id).map_err(to_error)?;
    let path = flowstate_db::task_plan_path(&id);
    std::fs::create_dir_all(path.parent().unwrap())
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(format!("mkdir: {e}"))))?;
    std::fs::write(&path, &body)
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(format!("write: {e}"))))?;

    // Auto-set plan_status to Pending if currently None and content is non-empty
    if task.plan_status == ApprovalStatus::None && !body.trim().is_empty() {
        let update = UpdateTask {
            plan_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update);
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn list_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state.service.list_attachments(&id)
        .map(|a| Json(json!(a)))
        .map_err(to_error)
}

fn sha256_hex(content: &str) -> String {
    let mut h = Sha256::new();
    h.update(content.as_bytes());
    format!("{:x}", h.finalize())
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
