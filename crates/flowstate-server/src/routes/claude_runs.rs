use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus, CreateClaudeRun};
use flowstate_core::runner::RunnerCapability;
use flowstate_service::TaskService;
use serde::Deserialize;
use serde_json::{json, Value};

use super::{AppState, RunnerInfo};

pub fn routes() -> Router<AppState> {
    Router::new()
        .route(
            "/api/tasks/{task_id}/claude-runs",
            get(list_claude_runs).post(trigger_claude_run),
        )
        .route("/api/claude-runs/claim", post(claim_claude_run))
        .route("/api/claude-runs/{id}", get(get_claude_run))
        .route(
            "/api/claude-runs/{id}/status",
            put(update_claude_run_status),
        )
        .route(
            "/api/claude-runs/{id}/progress",
            put(update_claude_run_progress),
        )
        .route("/api/claude-runs/{id}/output", get(get_claude_run_output))
        .route("/api/runners/register", post(register_runner))
}

#[derive(Debug, Deserialize)]
struct TriggerInput {
    action: String,
    #[serde(default)]
    required_capability: Option<String>,
}

/// Validate that prerequisites are met for triggering a Claude run
/// with the given action on the given task.
///
/// `has_completed_build`: whether any ClaudeRun with action=Build
///     and status=Completed exists for this task
/// `has_prs`: whether any TaskPrs are linked to this task
///
/// Returns Ok(()) if the action can proceed, or Err with a human-readable message.
pub(crate) fn validate_action_prerequisites(
    action: ClaudeAction,
    task: &flowstate_core::task::Task,
    has_completed_build: bool,
    has_prs: bool,
) -> Result<(), String> {
    // Research: no prerequisites

    // ResearchDistill: research artifact must exist
    if action == ClaudeAction::ResearchDistill
        && task.research_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err("cannot distill research: research artifact must exist first".to_string());
    }

    // Design: research must be approved
    if action == ClaudeAction::Design
        && task.research_status != flowstate_core::task::ApprovalStatus::Approved
    {
        return Err(format!(
            "cannot design: research must be approved first (current: {})",
            task.research_status.display_name()
        ));
    }

    // DesignDistill: spec artifact must exist
    if action == ClaudeAction::DesignDistill
        && task.spec_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err("cannot distill design: spec artifact must exist first".to_string());
    }

    // Spec must be approved before planning
    if action == ClaudeAction::Plan
        && task.spec_status != flowstate_core::task::ApprovalStatus::Approved
    {
        return Err(format!(
            "cannot plan: spec must be approved first (current: {})",
            task.spec_status.display_name()
        ));
    }

    // PlanDistill: plan artifact must exist
    if action == ClaudeAction::PlanDistill
        && task.plan_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err("cannot distill plan: plan artifact must exist first".to_string());
    }

    // Both spec and plan must be approved before building
    if action == ClaudeAction::Build {
        if task.spec_status != flowstate_core::task::ApprovalStatus::Approved {
            return Err(format!(
                "cannot build: spec must be approved first (current: {})",
                task.spec_status.display_name()
            ));
        }
        if task.plan_status != flowstate_core::task::ApprovalStatus::Approved {
            return Err(format!(
                "cannot build: plan must be approved first (current: {})",
                task.plan_status.display_name()
            ));
        }
    }

    // Verify: build must be completed or a PR must be linked
    if action == ClaudeAction::Verify && !has_completed_build && !has_prs {
        return Err(
            "cannot verify: build must be completed or a PR must be linked first".to_string(),
        );
    }

    // VerifyDistill: verify artifact must exist
    if action == ClaudeAction::VerifyDistill
        && task.verify_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err(
            "cannot distill verification: verification artifact must exist first".to_string(),
        );
    }

    Ok(())
}

async fn trigger_claude_run(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
    Json(input): Json<TriggerInput>,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    let action = ClaudeAction::parse_str(&input.action).ok_or_else(|| {
        to_error(flowstate_service::ServiceError::InvalidInput(format!(
            "invalid action: {} (expected research, design, plan, build, verify, research_distill, design_distill, plan_distill, or verify_distill)",
            input.action
        )))
    })?;

    let task = state.service.get_task(&task_id).await.map_err(to_error)?;

    // For Verify, look up build/PR status
    let (has_completed_build, has_prs) = if action == ClaudeAction::Verify {
        let runs = state
            .service
            .list_claude_runs(&task_id)
            .await
            .map_err(to_error)?;
        let prs = state
            .service
            .list_task_prs(&task_id)
            .await
            .map_err(to_error)?;
        (
            runs.iter()
                .any(|r| r.action == ClaudeAction::Build && r.status == ClaudeRunStatus::Completed),
            !prs.is_empty(),
        )
    } else {
        (false, false)
    };

    validate_action_prerequisites(action, &task, has_completed_build, has_prs)
        .map_err(|msg| to_error(flowstate_service::ServiceError::InvalidInput(msg)))?;

    let cap = input
        .required_capability
        .and_then(|c| RunnerCapability::parse_str(&c))
        .or_else(|| task.capability_for_action(action))
        .unwrap_or_else(|| RunnerCapability::default_for_action(action));
    let required_capability = Some(cap.as_str().to_string());
    let create = CreateClaudeRun {
        task_id: task_id.clone(),
        action,
        required_capability,
    };
    let run = state
        .service
        .create_claude_run(&create)
        .await
        .map_err(to_error)?;

    // The runner will pick this up via polling — no tokio::spawn here.

    Ok((StatusCode::CREATED, Json(json!(run))))
}

/// Claim the oldest queued run, atomically setting it to Running.
/// Returns 204 if no queued runs exist.
/// Also records the runner heartbeat via X-Runner-Id header.
/// If the runner is registered, uses its capability tiers for filtering.
async fn claim_claude_run(
    State(state): State<AppState>,
    headers: HeaderMap,
) -> Result<(StatusCode, Json<Value>), (StatusCode, Json<Value>)> {
    // Record runner heartbeat
    let runner_id = headers
        .get("X-Runner-Id")
        .and_then(|v| v.to_str().ok())
        .unwrap_or("unknown")
        .to_string();

    // Look up registered capabilities for this runner
    let capabilities: Vec<String> = {
        let runners = state.runners.lock().unwrap();
        runners
            .get(&runner_id)
            .map(|info| info.capabilities.clone())
            .unwrap_or_default()
    };

    // Update last_seen (preserve existing registration info)
    {
        let mut runners = state.runners.lock().unwrap();
        runners
            .entry(runner_id.clone())
            .and_modify(|info| info.last_seen = Utc::now())
            .or_insert_with(|| RunnerInfo {
                runner_id: runner_id.clone(),
                last_seen: Utc::now(),
                backend_name: None,
                capability: None,
                capabilities: vec![],
                poll_interval: None,
                max_concurrent: None,
                max_builds: None,
                active_count: None,
                active_builds: None,
                status: super::RunnerStatus::Active,
                pending_config: None,
            });
    }

    let cap_refs: Vec<&str> = capabilities.iter().map(|s| s.as_str()).collect();
    let result = state
        .db
        .claim_next_claude_run(&cap_refs)
        .await
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(e.to_string())))?;

    match result {
        Some(run) => {
            // Record which runner claimed this run
            let _ = state.db.set_claude_run_runner(&run.id, &runner_id).await;
            Ok((StatusCode::OK, Json(json!(run))))
        }
        None => Ok((StatusCode::NO_CONTENT, Json(json!(null)))),
    }
}

#[derive(Debug, Deserialize)]
struct RegisterRunnerInput {
    runner_id: String,
    #[serde(default)]
    backend_name: Option<String>,
    #[serde(default)]
    capability: Option<String>,
    #[serde(default)]
    poll_interval: Option<u64>,
    #[serde(default)]
    max_concurrent: Option<usize>,
    #[serde(default)]
    max_builds: Option<usize>,
    #[serde(default)]
    active_count: Option<usize>,
    #[serde(default)]
    active_builds: Option<usize>,
    #[serde(default)]
    status: Option<String>,
}

/// Register a runner with the server, recording its capabilities.
/// Also serves as a heartbeat: runner calls this at the top of each poll iteration.
/// Returns any pending config changes for the runner.
async fn register_runner(
    State(state): State<AppState>,
    Json(input): Json<RegisterRunnerInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    // Parse capability and compute handled tiers
    let capabilities: Vec<String> = input
        .capability
        .as_deref()
        .and_then(RunnerCapability::parse_str)
        .map(|cap| {
            cap.handled_tiers()
                .into_iter()
                .map(|t| t.as_str().to_string())
                .collect()
        })
        .unwrap_or_default();

    // Parse runner status from input
    let runner_status = input
        .status
        .as_deref()
        .map(|s| match s {
            "draining" => super::RunnerStatus::Draining,
            "drained" => super::RunnerStatus::Drained,
            _ => super::RunnerStatus::Active,
        })
        .unwrap_or(super::RunnerStatus::Active);

    // Extract pending config to return, then clear it
    let pending_config = {
        let mut runners = state.runners.lock().unwrap();
        let existing_pending = runners
            .get(&input.runner_id)
            .and_then(|r| r.pending_config.clone());

        let info = RunnerInfo {
            runner_id: input.runner_id.clone(),
            last_seen: Utc::now(),
            backend_name: input.backend_name.clone(),
            capability: input.capability.clone(),
            capabilities,
            poll_interval: input.poll_interval,
            max_concurrent: input.max_concurrent,
            max_builds: input.max_builds,
            active_count: input.active_count,
            active_builds: input.active_builds,
            status: runner_status,
            pending_config: None, // cleared after delivery
        };

        runners.insert(input.runner_id.clone(), info);
        existing_pending
    };

    Ok(Json(json!({
        "status": "registered",
        "runner_id": input.runner_id,
        "pending_config": pending_config,
    })))
}

#[derive(Debug, Deserialize)]
struct UpdateStatusInput {
    status: String,
    #[serde(default)]
    error_message: Option<String>,
    #[serde(default)]
    exit_code: Option<i32>,
    #[serde(default)]
    pr_url: Option<String>,
    #[serde(default)]
    pr_number: Option<i64>,
    #[serde(default)]
    branch_name: Option<String>,
}

async fn update_claude_run_status(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<UpdateStatusInput>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    let status = ClaudeRunStatus::parse_str(&input.status).ok_or_else(|| {
        to_error(flowstate_service::ServiceError::InvalidInput(format!(
            "invalid status: {}",
            input.status
        )))
    })?;

    let run = state
        .db
        .update_claude_run_status(&id, status, input.error_message.as_deref(), input.exit_code)
        .await
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(e.to_string())))?;

    // Update PR info if provided
    if input.pr_url.is_some() || input.pr_number.is_some() || input.branch_name.is_some() {
        let run = state
            .db
            .update_claude_run_pr(
                &id,
                input.pr_url.as_deref(),
                input.pr_number,
                input.branch_name.as_deref(),
            )
            .await
            .map_err(|e| to_error(flowstate_service::ServiceError::Internal(e.to_string())))?;
        return Ok(Json(json!(run)));
    }

    Ok(Json(json!(run)))
}

#[derive(Debug, Deserialize)]
struct ProgressInput {
    message: String,
}

async fn update_claude_run_progress(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(input): Json<ProgressInput>,
) -> Result<StatusCode, (StatusCode, Json<Value>)> {
    state
        .db
        .update_claude_run_progress(&id, &input.message)
        .await
        .map_err(|e| to_error(flowstate_service::ServiceError::Internal(e.to_string())))?;
    Ok(StatusCode::NO_CONTENT)
}

async fn list_claude_runs(
    State(state): State<AppState>,
    Path(task_id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .list_claude_runs(&task_id)
        .await
        .map(|r| Json(json!(r)))
        .map_err(to_error)
}

async fn get_claude_run(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<Value>, (StatusCode, Json<Value>)> {
    state
        .service
        .get_claude_run(&id)
        .await
        .map(|r| Json(json!(r)))
        .map_err(to_error)
}

async fn get_claude_run_output(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<String, (StatusCode, Json<Value>)> {
    // First verify the run exists
    let _run = state.service.get_claude_run(&id).await.map_err(to_error)?;

    let key = flowstate_store::claude_run_output_key(&id);
    match state.store.get_opt(&key).await {
        Ok(Some(data)) => {
            let content = String::from_utf8_lossy(&data);
            Ok(content.into_owned())
        }
        Ok(None) => Err(to_error(flowstate_service::ServiceError::NotFound(
            "output not yet available".into(),
        ))),
        Err(e) => Err(to_error(flowstate_service::ServiceError::Internal(
            format!("read output: {e}"),
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

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::task::{ApprovalStatus, Priority, Status, Task};

    fn make_test_task() -> Task {
        Task {
            id: "task-1".into(),
            project_id: "proj-1".into(),
            sprint_id: None,
            parent_id: None,
            title: "Test".into(),
            description: String::new(),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
            research_status: ApprovalStatus::None,
            spec_status: ApprovalStatus::None,
            plan_status: ApprovalStatus::None,
            verify_status: ApprovalStatus::None,
            spec_approved_hash: String::new(),
            research_approved_hash: String::new(),
            research_feedback: String::new(),
            spec_feedback: String::new(),
            plan_feedback: String::new(),
            verify_feedback: String::new(),
            status: Status::Todo,
            priority: Priority::Medium,
            sort_order: 1.0,
            created_at: chrono::Utc::now(),
            updated_at: chrono::Utc::now(),
        }
    }

    #[test]
    fn test_prerequisites_research_always_passes() {
        let task = make_test_task();
        assert!(validate_action_prerequisites(ClaudeAction::Research, &task, false, false).is_ok());
    }

    #[test]
    fn test_prerequisites_research_distill_needs_research() {
        let mut task = make_test_task();
        assert!(
            validate_action_prerequisites(ClaudeAction::ResearchDistill, &task, false, false)
                .is_err()
        );
        task.research_status = ApprovalStatus::Pending;
        assert!(
            validate_action_prerequisites(ClaudeAction::ResearchDistill, &task, false, false)
                .is_ok()
        );
    }

    #[test]
    fn test_prerequisites_design_needs_approved_research() {
        let mut task = make_test_task();
        task.research_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::Design, &task, false, false).is_err());
        task.research_status = ApprovalStatus::Approved;
        assert!(validate_action_prerequisites(ClaudeAction::Design, &task, false, false).is_ok());
    }

    #[test]
    fn test_prerequisites_design_distill_needs_spec() {
        let mut task = make_test_task();
        assert!(
            validate_action_prerequisites(ClaudeAction::DesignDistill, &task, false, false)
                .is_err()
        );
        task.spec_status = ApprovalStatus::Pending;
        assert!(
            validate_action_prerequisites(ClaudeAction::DesignDistill, &task, false, false).is_ok()
        );
    }

    #[test]
    fn test_prerequisites_plan_needs_approved_spec() {
        let mut task = make_test_task();
        task.spec_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::Plan, &task, false, false).is_err());
        task.spec_status = ApprovalStatus::Approved;
        assert!(validate_action_prerequisites(ClaudeAction::Plan, &task, false, false).is_ok());
    }

    #[test]
    fn test_prerequisites_plan_distill_needs_plan() {
        let mut task = make_test_task();
        assert!(
            validate_action_prerequisites(ClaudeAction::PlanDistill, &task, false, false).is_err()
        );
        task.plan_status = ApprovalStatus::Pending;
        assert!(
            validate_action_prerequisites(ClaudeAction::PlanDistill, &task, false, false).is_ok()
        );
    }

    #[test]
    fn test_prerequisites_build_needs_approved_spec_and_plan() {
        let mut task = make_test_task();
        // Only spec approved
        task.spec_status = ApprovalStatus::Approved;
        assert!(validate_action_prerequisites(ClaudeAction::Build, &task, false, false).is_err());
        // Only plan approved
        task.spec_status = ApprovalStatus::None;
        task.plan_status = ApprovalStatus::Approved;
        assert!(validate_action_prerequisites(ClaudeAction::Build, &task, false, false).is_err());
        // Both approved
        task.spec_status = ApprovalStatus::Approved;
        assert!(validate_action_prerequisites(ClaudeAction::Build, &task, false, false).is_ok());
    }

    #[test]
    fn test_prerequisites_verify_needs_build_or_pr() {
        let task = make_test_task();
        // Neither
        assert!(validate_action_prerequisites(ClaudeAction::Verify, &task, false, false).is_err());
        // Build only
        assert!(validate_action_prerequisites(ClaudeAction::Verify, &task, true, false).is_ok());
        // PR only
        assert!(validate_action_prerequisites(ClaudeAction::Verify, &task, false, true).is_ok());
    }

    #[test]
    fn test_prerequisites_verify_distill_needs_verification() {
        let mut task = make_test_task();
        assert!(
            validate_action_prerequisites(ClaudeAction::VerifyDistill, &task, false, false)
                .is_err()
        );
        task.verify_status = ApprovalStatus::Pending;
        assert!(
            validate_action_prerequisites(ClaudeAction::VerifyDistill, &task, false, false).is_ok()
        );
    }

    // ---- Integration tests ----

    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode as AxumStatusCode};
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    /// Helper: create a task and return its id.
    async fn create_task(app: &axum::Router, project_id: &str) -> String {
        let body = serde_json::to_string(&json!({
            "project_id": project_id,
            "title": "Run Task",
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
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let v: Value = serde_json::from_slice(&bytes).unwrap();
        v["id"].as_str().unwrap().to_string()
    }

    #[tokio::test]
    async fn claim_claude_run_empty() {
        let app = test_router().await;
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/claude-runs/claim")
                    .header("X-Runner-Id", "test-runner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn claim_claude_run_success() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger a research run (no prerequisites)
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::CREATED);

        // Claim the run
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/claude-runs/claim")
                    .header("X-Runner-Id", "test-runner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let run: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(run["status"], "running");
    }

    #[tokio::test]
    async fn get_claude_run_by_id() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let run_id = created["id"].as_str().unwrap();

        // Get by ID
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/claude-runs/{run_id}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let run: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(run["id"], run_id);
    }

    #[tokio::test]
    async fn update_claude_run_status_flow() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let run_id = created["id"].as_str().unwrap();

        // Claim to set Running
        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/claude-runs/claim")
                    .header("X-Runner-Id", "test-runner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Update status to completed
        let body = serde_json::to_string(&json!({
            "status": "completed",
            "exit_code": 0
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/claude-runs/{run_id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let run: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(run["status"], "completed");
    }

    #[tokio::test]
    async fn update_claude_run_progress_flow() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger + claim
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let run_id = created["id"].as_str().unwrap();

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/claude-runs/claim")
                    .header("X-Runner-Id", "test-runner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Update progress
        let body = serde_json::to_string(&json!({"message": "Cloning..."})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/claude-runs/{run_id}/progress"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::NO_CONTENT);
    }

    #[tokio::test]
    async fn update_claude_run_status_with_pr_info() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger + claim
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let run_id = created["id"].as_str().unwrap();

        let _ = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/claude-runs/claim")
                    .header("X-Runner-Id", "test-runner")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        // Update status with PR info
        let body = serde_json::to_string(&json!({
            "status": "completed",
            "pr_url": "https://github.com/test/repo/pull/1",
            "pr_number": 1,
            "branch_name": "flowstate/test"
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::PUT)
                    .uri(format!("/api/claude-runs/{run_id}/status"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let run: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(run["pr_url"], "https://github.com/test/repo/pull/1");
    }

    #[tokio::test]
    async fn register_runner_with_capabilities() {
        let app = test_router().await;

        let body = serde_json::to_string(&json!({
            "runner_id": "cap-runner-1",
            "backend_name": "claude-cli",
            "capability": "heavy",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/api/runners/register")
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let result: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(result["status"], "registered");
        assert_eq!(result["runner_id"], "cap-runner-1");
    }

    #[tokio::test]
    async fn get_claude_run_output_missing() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // Trigger
        let body = serde_json::to_string(&json!({"action": "research"})).unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let created: Value = serde_json::from_slice(&bytes).unwrap();
        let run_id = created["id"].as_str().unwrap();

        // Get output (none exists)
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/claude-runs/{run_id}/output"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::NOT_FOUND);
    }

    #[tokio::test]
    async fn trigger_and_list_runs() {
        let app = test_router().await;
        let project_id = create_project(&app).await;
        let task_id = create_task(&app, &project_id).await;

        // POST /api/tasks/{task_id}/claude-runs with action=research (no prerequisites)
        let body = serde_json::to_string(&json!({
            "action": "research",
        }))
        .unwrap();
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .header("content-type", "application/json")
                    .body(Body::from(body))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::CREATED);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let run: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(run["task_id"].as_str().unwrap(), task_id);
        assert_eq!(run["action"], "research");

        // GET /api/tasks/{task_id}/claude-runs → list has 1 item
        let resp = app
            .clone()
            .oneshot(
                Request::builder()
                    .uri(format!("/api/tasks/{task_id}/claude-runs"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(resp.status(), AxumStatusCode::OK);
        let bytes = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let list: Value = serde_json::from_slice(&bytes).unwrap();
        assert_eq!(list.as_array().unwrap().len(), 1);
    }
}
