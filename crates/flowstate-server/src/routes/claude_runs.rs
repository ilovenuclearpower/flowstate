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
        .route("/api/claude-runs/{id}/status", put(update_claude_run_status))
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
    if action == ClaudeAction::Verify {
        if !has_completed_build && !has_prs {
            return Err(
                "cannot verify: build must be completed or a PR must be linked first".to_string(),
            );
        }
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
        let runs = state.service.list_claude_runs(&task_id).await.map_err(to_error)?;
        let prs = state.service.list_task_prs(&task_id).await.map_err(to_error)?;
        (
            runs.iter().any(|r| {
                r.action == ClaudeAction::Build && r.status == ClaudeRunStatus::Completed
            }),
            !prs.is_empty(),
        )
    } else {
        (false, false)
    };

    validate_action_prerequisites(action, &task, has_completed_build, has_prs)
        .map_err(|msg| to_error(flowstate_service::ServiceError::InvalidInput(msg)))?;

    let required_capability = Some(
        RunnerCapability::default_for_action(action).as_str().to_string(),
    );
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
            });
    }

    let cap_refs: Vec<&str> = capabilities.iter().map(|s| s.as_str()).collect();
    let result = state
        .db
        .claim_next_claude_run(&cap_refs)
        .await
        .map_err(|e| {
            to_error(flowstate_service::ServiceError::Internal(e.to_string()))
        })?;

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
}

/// Register a runner with the server, recording its capabilities.
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

    let info = RunnerInfo {
        runner_id: input.runner_id.clone(),
        last_seen: Utc::now(),
        backend_name: input.backend_name.clone(),
        capability: input.capability.clone(),
        capabilities,
    };

    state
        .runners
        .lock()
        .unwrap()
        .insert(input.runner_id.clone(), info);

    Ok(Json(json!({
        "status": "registered",
        "runner_id": input.runner_id,
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
        .update_claude_run_status(
            &id,
            status,
            input.error_message.as_deref(),
            input.exit_code,
        )
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
        assert!(validate_action_prerequisites(ClaudeAction::ResearchDistill, &task, false, false).is_err());
        task.research_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::ResearchDistill, &task, false, false).is_ok());
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
        assert!(validate_action_prerequisites(ClaudeAction::DesignDistill, &task, false, false).is_err());
        task.spec_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::DesignDistill, &task, false, false).is_ok());
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
        assert!(validate_action_prerequisites(ClaudeAction::PlanDistill, &task, false, false).is_err());
        task.plan_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::PlanDistill, &task, false, false).is_ok());
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
        assert!(validate_action_prerequisites(ClaudeAction::VerifyDistill, &task, false, false).is_err());
        task.verify_status = ApprovalStatus::Pending;
        assert!(validate_action_prerequisites(ClaudeAction::VerifyDistill, &task, false, false).is_ok());
    }
}
