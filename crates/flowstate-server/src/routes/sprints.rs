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

#[cfg(test)]
mod tests {
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use tower::ServiceExt;
    use crate::test_helpers::test_router;

    #[tokio::test]
    async fn sprint_crud_lifecycle() {
        let app = test_router().await;

        // Create a project first (sprints require a project_id)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/projects")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": "Test Project",
                            "slug": "test-project"
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

        // POST /api/sprints → 201
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/sprints")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "project_id": project_id,
                            "name": "Sprint 1",
                            "goal": "First sprint"
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
        let sprint: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let sprint_id = sprint["id"].as_str().unwrap();
        assert_eq!(sprint["name"], "Sprint 1");

        // GET /api/sprints/{id}
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/sprints/{sprint_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let fetched: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(fetched["name"], "Sprint 1");

        // PUT /api/sprints/{id}
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/sprints/{sprint_id}"))
                    .header("content-type", "application/json")
                    .body(Body::from(
                        serde_json::to_vec(&serde_json::json!({
                            "name": "Sprint 1 Updated"
                        }))
                        .unwrap(),
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let updated: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(updated["name"], "Sprint 1 Updated");

        // DELETE /api/sprints/{id} → 204
        let resp = app
            .oneshot(
                Request::builder()
                    .method(Method::DELETE)
                    .uri(format!("/api/sprints/{sprint_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), StatusCode::NO_CONTENT);
    }
}
