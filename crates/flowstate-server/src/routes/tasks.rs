use axum::{
    body::Body,
    extract::{Path, Query, State},
    http::StatusCode,
    response::Response,
    routing::get,
    Json, Router,
};
use bytes::Bytes;
use flowstate_core::task::{
    self, ApprovalStatus, CreateTask, Priority, Status, TaskFilter, UpdateTask,
};
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
        .route(
            "/api/tasks/{id}/research",
            get(read_research).put(write_research),
        )
        .route(
            "/api/tasks/{id}/verification",
            get(read_verification).put(write_verification),
        )
        .route(
            "/api/tasks/{id}/feedback",
            axum::routing::put(write_feedback),
        )
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
        status: q.status.and_then(|s| Status::parse_str(&s)),
        priority: q.priority.and_then(|p| Priority::parse_str(&p)),
        sprint_id: q.sprint_id,
        parent_id: None,
        limit: q.limit,
    };
    state
        .service
        .list_tasks(&filter)
        .await
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn get_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .get_task(&id)
        .await
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn create_task(
    State(state): State<AppState>,
    Json(input): Json<CreateTask>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    state
        .service
        .create_task(&input)
        .await
        .map(|t| (StatusCode::CREATED, Json(json!(t))))
        .map_err(to_error)
}

async fn update_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(mut input): Json<UpdateTask>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Fetch current task for status comparison and hash logic
    let current_task = state.service.get_task(&id).await.map_err(to_error)?;

    // On spec approval, compute and store the spec content hash
    if input.spec_status == Some(ApprovalStatus::Approved) {
        let key = flowstate_store::task_spec_key(&id);
        if let Ok(Some(data)) = state.store.get_opt(&key).await {
            let content = String::from_utf8_lossy(&data);
            input.spec_approved_hash = Some(sha256_hex(&content));
        }
    }
    // On research approval, compute and store the research content hash
    if input.research_status == Some(ApprovalStatus::Approved) {
        let key = flowstate_store::task_research_key(&id);
        if let Ok(Some(data)) = state.store.get_opt(&key).await {
            let content = String::from_utf8_lossy(&data);
            input.research_approved_hash = Some(sha256_hex(&content));
        }
    }

    // Auto-advance board status on approval (forward-only, skip Cancelled)
    if input.status.is_none() && current_task.status != Status::Cancelled {
        let target = if input.research_status == Some(ApprovalStatus::Approved) {
            task::status_after_approval("research")
        } else if input.spec_status == Some(ApprovalStatus::Approved) {
            task::status_after_approval("spec")
        } else if input.plan_status == Some(ApprovalStatus::Approved) {
            task::status_after_approval("plan")
        } else if input.verify_status == Some(ApprovalStatus::Approved) {
            task::status_after_approval("verify")
        } else {
            None
        };

        if let Some(next) = target {
            if next.ordinal() > current_task.status.ordinal() {
                input.status = Some(next);
            }
        }
    }

    state
        .service
        .update_task(&id, &input)
        .await
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn delete_task(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state
        .service
        .delete_task(&id)
        .await
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
    state
        .service
        .count_tasks_by_status(&q.project_id)
        .await
        .map(|c| Json(json!(c)))
        .map_err(to_error)
}

async fn list_children(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_child_tasks(&id)
        .await
        .map(|t| Json(json!(t)))
        .map_err(to_error)
}

async fn read_spec(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    // Verify task exists
    let _task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_spec_key(&id);
    match state.store.get_opt(&key).await {
        Ok(Some(data)) => {
            let content = String::from_utf8_lossy(&data);
            Ok(Response::builder()
                .header("Content-Type", "text/markdown")
                .body(Body::from(content.into_owned()))
                .unwrap())
        }
        Ok(None) => Ok(Response::builder()
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
    let task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_spec_key(&id);
    state
        .store
        .put(&key, Bytes::from(body.clone()))
        .await
        .map_err(|e| {
            to_error(flowstate_service::ServiceError::Internal(format!(
                "write: {e}"
            )))
        })?;

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
            let _ = state.service.update_task(&id, &update).await;
        }
    } else if task.spec_status == ApprovalStatus::None && !body.trim().is_empty() {
        // Auto-set to Pending when content written for the first time
        let update = UpdateTask {
            spec_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn read_plan(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let _task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_plan_key(&id);
    match state.store.get_opt(&key).await {
        Ok(Some(data)) => {
            let content = String::from_utf8_lossy(&data);
            Ok(Response::builder()
                .header("Content-Type", "text/markdown")
                .body(Body::from(content.into_owned()))
                .unwrap())
        }
        Ok(None) => Ok(Response::builder()
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
    let task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_plan_key(&id);
    state
        .store
        .put(&key, Bytes::from(body.clone()))
        .await
        .map_err(|e| {
            to_error(flowstate_service::ServiceError::Internal(format!(
                "write: {e}"
            )))
        })?;

    // Auto-set plan_status to Pending if currently None and content is non-empty
    if task.plan_status == ApprovalStatus::None && !body.trim().is_empty() {
        let update = UpdateTask {
            plan_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn read_research(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let _task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_research_key(&id);
    match state.store.get_opt(&key).await {
        Ok(Some(data)) => {
            let content = String::from_utf8_lossy(&data);
            Ok(Response::builder()
                .header("Content-Type", "text/markdown")
                .body(Body::from(content.into_owned()))
                .unwrap())
        }
        Ok(None) => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(""))
            .unwrap()),
        Err(e) => Err(to_error(flowstate_service::ServiceError::Internal(
            format!("read research: {e}"),
        ))),
    }
}

async fn write_research(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_research_key(&id);
    state
        .store
        .put(&key, Bytes::from(body.clone()))
        .await
        .map_err(|e| {
            to_error(flowstate_service::ServiceError::Internal(format!(
                "write: {e}"
            )))
        })?;

    // Server-side status management
    if task.research_status == ApprovalStatus::Approved && !task.research_approved_hash.is_empty() {
        // Revoke approval if content changed
        let new_hash = sha256_hex(&body);
        if new_hash != task.research_approved_hash {
            let update = UpdateTask {
                research_status: Some(ApprovalStatus::Pending),
                research_approved_hash: Some(String::new()),
                ..Default::default()
            };
            let _ = state.service.update_task(&id, &update).await;
        }
    } else if task.research_status == ApprovalStatus::None && !body.trim().is_empty() {
        // Auto-set to Pending when content written for the first time
        let update = UpdateTask {
            research_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

async fn read_verification(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Response, (StatusCode, Json<Value>)> {
    let _task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_verification_key(&id);
    match state.store.get_opt(&key).await {
        Ok(Some(data)) => {
            let content = String::from_utf8_lossy(&data);
            Ok(Response::builder()
                .header("Content-Type", "text/markdown")
                .body(Body::from(content.into_owned()))
                .unwrap())
        }
        Ok(None) => Ok(Response::builder()
            .header("Content-Type", "text/markdown")
            .body(Body::from(""))
            .unwrap()),
        Err(e) => Err(to_error(flowstate_service::ServiceError::Internal(
            format!("read verification: {e}"),
        ))),
    }
}

async fn write_verification(
    State(state): State<AppState>,
    Path(id): Path<String>,
    body: String,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let task = state.service.get_task(&id).await.map_err(to_error)?;
    let key = flowstate_store::task_verification_key(&id);
    state
        .store
        .put(&key, Bytes::from(body.clone()))
        .await
        .map_err(|e| {
            to_error(flowstate_service::ServiceError::Internal(format!(
                "write: {e}"
            )))
        })?;

    // Auto-set verify_status to Pending if currently None and content is non-empty
    if task.verify_status == ApprovalStatus::None && !body.trim().is_empty() {
        let update = UpdateTask {
            verify_status: Some(ApprovalStatus::Pending),
            ..Default::default()
        };
        let _ = state.service.update_task(&id, &update).await;
    }

    Ok(StatusCode::NO_CONTENT)
}

#[derive(Debug, Deserialize)]
struct FeedbackInput {
    phase: String,
    feedback: String,
}

async fn write_feedback(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<FeedbackInput>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    let _task = state.service.get_task(&id).await.map_err(to_error)?;
    let update = match input.phase.as_str() {
        "research" => UpdateTask {
            research_feedback: Some(input.feedback),
            ..Default::default()
        },
        "design" => UpdateTask {
            spec_feedback: Some(input.feedback),
            ..Default::default()
        },
        "plan" => UpdateTask {
            plan_feedback: Some(input.feedback),
            ..Default::default()
        },
        "verify" => UpdateTask {
            verify_feedback: Some(input.feedback),
            ..Default::default()
        },
        _ => {
            return Err(to_error(flowstate_service::ServiceError::InvalidInput(
                format!("invalid phase: {}", input.phase),
            )))
        }
    };
    state
        .service
        .update_task(&id, &update)
        .await
        .map_err(to_error)?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_attachments(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_attachments(&id)
        .await
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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::{json, Value};
    use tower::ServiceExt;

    use crate::test_helpers::test_router;

    /// Helper: create a project and return its id.
    async fn create_project(app: &axum::Router) -> String {
        let body = serde_json::to_string(&json!({
            "name": "Test Project",
            "slug": "test-proj",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn task_crud_lifecycle() {
        let app = test_router().await;
        let project_id = create_project(&app).await;

        // POST /api/tasks → 201
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "My Task",
            "description": "Task desc",
            "status": "todo",
            "priority": "medium",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let task_id = created["id"].as_str().unwrap();
        assert_eq!(created["title"], "My Task");

        // GET /api/tasks/{id}
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fetched: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(fetched["title"], "My Task");

        // PUT /api/tasks/{id}
        let body = serde_json::to_string(&json!({
            "title": "Updated Task",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(updated["title"], "Updated Task");

        // DELETE /api/tasks/{id} → 204
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn task_list_with_filters() {
        let app = test_router().await;
        let project_id = create_project(&app).await;

        // Create task with status=todo
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Todo Task",
            "status": "todo",
            "priority": "medium",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // Create task with status=done
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Done Task",
            "status": "done",
            "priority": "high",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // GET /api/tasks?project_id=X&status=todo → only 1
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks?project_id={project_id}&status=todo"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = list.as_array().unwrap();
        assert_eq!(arr.len(), 1);
        assert_eq!(arr[0]["title"], "Todo Task");
    }

    #[tokio::test]
    async fn task_spec_write_and_read() {
        let app = test_router().await;
        let project_id = create_project(&app).await;

        // Create task
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Spec Task",
            "status": "todo",
            "priority": "medium",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let task_id = created["id"].as_str().unwrap();

        // PUT /api/tasks/{id}/spec
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/spec"))
                    .body(Body::from("spec content here"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // GET /api/tasks/{id}/spec
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/spec"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "spec content here");
    }

    #[tokio::test]
    async fn task_plan_write_and_read() {
        let app = test_router().await;
        let project_id = create_project(&app).await;

        // Create task
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Plan Task",
            "status": "todo",
            "priority": "medium",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let task_id = created["id"].as_str().unwrap();

        // PUT /api/tasks/{id}/plan
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/plan"))
                    .body(Body::from("plan content here"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // GET /api/tasks/{id}/plan
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/plan"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "plan content here");
    }

    /// Helper: create a task and return its id.
    async fn create_task(app: &axum::Router, project_id: &str) -> String {
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Test Task",
            "status": "todo",
            "priority": "medium",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn count_by_status_endpoint() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let _task_id = create_task(&app, &project_id).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!(
                        "/api/tasks/count-by-status?project_id={project_id}"
                    ))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let counts: Value = serde_json::from_slice(&bytes).unwrap();
        let arr = counts.as_array().unwrap();
        // Should have at least one entry for "todo"
        assert!(arr.iter().any(|c| c[0] == "todo"));
    }

    #[tokio::test]
    async fn list_children_endpoint() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let parent_id = create_task(&app, &project_id).await;

        // Create a child task
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Child Task",
            "status": "todo",
            "priority": "low",
            "parent_id": parent_id,
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);

        // GET /api/tasks/{id}/children
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{parent_id}/children"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let children: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(children.as_array().unwrap().len(), 1);
        assert_eq!(children[0]["title"], "Child Task");
    }

    #[tokio::test]
    async fn research_write_and_read() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Write research
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/research"))
                    .body(Body::from("research findings"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Read research
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/research"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "research findings");
    }

    #[tokio::test]
    async fn verification_write_and_read() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Write verification
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/verification"))
                    .body(Body::from("verification steps"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Read verification
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/verification"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "verification steps");
    }

    #[tokio::test]
    async fn write_spec_revokes_approval_on_change() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Write spec
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/spec"))
                    .body(Body::from("original spec"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Approve spec
        let body = serde_json::to_string(&json!({
            "spec_status": "approved",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);

        // Verify it's approved
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let task: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(task["spec_status"], "approved");

        // Write different spec content → should revoke approval
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/spec"))
                    .body(Body::from("changed spec content"))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);

        // Verify approval is revoked
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let task: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(task["spec_status"], "pending");
    }

    #[tokio::test]
    async fn write_feedback_routes() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Write feedback for each phase
        for phase in &["research", "design", "plan", "verify"] {
            let body = serde_json::to_string(&json!({
                "phase": phase,
                "feedback": format!("{phase} feedback text"),
            }))
            .unwrap();
            let resp = app
                .clone()
                .oneshot(
                    Request::builder()
                        .method(Method::PUT)
                        .uri(format!("/api/tasks/{task_id}/feedback"))
                        .header("content-type", "application/json")
                        .body(Body::from(body))
                        .unwrap(),
                )
                .await
                .unwrap();
            assert_eq!(resp.status(), StatusCode::NO_CONTENT);
        }

        // Verify task has feedback
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let task: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(task["research_feedback"], "research feedback text");
        assert_eq!(task["spec_feedback"], "design feedback text");
        assert_eq!(task["plan_feedback"], "plan feedback text");
        assert_eq!(task["verify_feedback"], "verify feedback text");

        // Invalid phase → 400
        let body = serde_json::to_string(&json!({
            "phase": "invalid",
            "feedback": "nope",
        }))
        .unwrap();
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/tasks/{task_id}/feedback"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn read_empty_content() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Read spec that doesn't exist yet → empty body
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/spec"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "");

        // Same for plan
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/plan"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "");

        // Same for research
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/research"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "");

        // Same for verification
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/verification"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        assert_eq!(std::str::from_utf8(&bytes).unwrap(), "");
    }

    #[tokio::test]
    async fn list_attachments_endpoint() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/attachments"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let attachments: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(attachments.as_array().unwrap().len(), 0);
    }
}
