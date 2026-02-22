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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;
    use crate::test_helpers::test_router;

    #[tokio::test]
    async fn task_pr_create_and_list() {
        let app = test_router().await;

        // Create project
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": "Test",
                            "slug": "test"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let project: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let project_id = project["id"].as_str().unwrap();

        // Create task
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/tasks")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "project_id": project_id,
                            "title": "Task 1",
                            "status": "todo",
                            "priority": "medium"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let task: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let task_id = task["id"].as_str().unwrap();

        // POST /api/tasks/{id}/prs → 201
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/prs"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "pr_url": "https://github.com/org/repo/pull/42",
                            "pr_number": 42,
                            "branch_name": "flowstate/test-branch"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::CREATED);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let pr: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(pr["pr_number"], 42);
        assert_eq!(pr["task_id"], task_id);

        // GET /api/tasks/{id}/prs
        let resp = app
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/prs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let prs: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(prs.as_array().unwrap().len(), 1);
        assert_eq!(prs[0]["pr_number"], 42);
    }
}
