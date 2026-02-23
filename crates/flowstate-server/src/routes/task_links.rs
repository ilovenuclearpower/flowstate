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
    state
        .service
        .create_task_link(&input)
        .await
        .map(|l| (StatusCode::CREATED, Json(json!(l))))
        .map_err(to_error)
}

async fn list_task_links(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_task_links(&task_id)
        .await
        .map(|l| Json(json!(l)))
        .map_err(to_error)
}

async fn delete_task_link(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state
        .service
        .delete_task_link(&id)
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

#[cfg(test)]
mod tests {
    use crate::test_helpers::test_router;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use axum::Router;
    use tower::ServiceExt;

    /// Helper: create a project and return its id.
    async fn create_project(app: &Router) -> String {
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
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    /// Helper: create a task and return its id.
    async fn create_task(app: &Router, project_id: &str) -> String {
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
                            "title": "Task",
                            "status": "todo",
                            "priority": "medium"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: serde_json::Value = serde_json::from_slice(&body).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn task_link_crud() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task1_id = create_task(&app, &project_id).await;
        let task2_id = create_task(&app, &project_id).await;

        // POST /api/task-links → 201
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/task-links")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "source_task_id": task1_id,
                            "target_task_id": task2_id,
                            "link_type": "blocks"
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
        let link: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let link_id = link["id"].as_str().unwrap();
        assert_eq!(link["source_task_id"], task1_id);
        assert_eq!(link["target_task_id"], task2_id);

        // GET /api/tasks/{task_id}/links
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task1_id}/links"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let links: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(links.as_array().unwrap().len(), 1);

        // DELETE /api/task-links/{id} → 204
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/task-links/{link_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
