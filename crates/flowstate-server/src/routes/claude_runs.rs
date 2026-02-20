use axum::{
    extract::{Path, State},
    http::{HeaderMap, StatusCode},
    routing::{get, post, put},
    Json, Router,
};
use chrono::Utc;
use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus, CreateClaudeRun};
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
    let action = ClaudeAction::parse_str(&input.action).ok_or_else(|| {
        to_error(flowstate_service::ServiceError::InvalidInput(format!(
            "invalid action: {} (expected research, design, plan, build, verify, research_distill, design_distill, plan_distill, or verify_distill)",
            input.action
        )))
    })?;

    let task = state.service.get_task(&task_id).await.map_err(to_error)?;

    // Research: no prerequisites

    // ResearchDistill: research artifact must exist
    if action == ClaudeAction::ResearchDistill
        && task.research_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            "cannot distill research: research artifact must exist first".to_string(),
        )));
    }

    // Design: research must be approved
    if action == ClaudeAction::Design
        && task.research_status != flowstate_core::task::ApprovalStatus::Approved
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            format!(
                "cannot design: research must be approved first (current: {})",
                task.research_status.display_name()
            ),
        )));
    }

    // DesignDistill: spec artifact must exist
    if action == ClaudeAction::DesignDistill
        && task.spec_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            "cannot distill design: spec artifact must exist first".to_string(),
        )));
    }

    // Spec must be approved before planning
    if action == ClaudeAction::Plan
        && task.spec_status != flowstate_core::task::ApprovalStatus::Approved
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            format!(
                "cannot plan: spec must be approved first (current: {})",
                task.spec_status.display_name()
            ),
        )));
    }

    // PlanDistill: plan artifact must exist
    if action == ClaudeAction::PlanDistill
        && task.plan_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            "cannot distill plan: plan artifact must exist first".to_string(),
        )));
    }

    // Both spec and plan must be approved before building
    if action == ClaudeAction::Build {
        if task.spec_status != flowstate_core::task::ApprovalStatus::Approved {
            return Err(to_error(flowstate_service::ServiceError::InvalidInput(
                format!(
                    "cannot build: spec must be approved first (current: {})",
                    task.spec_status.display_name()
                ),
            )));
        }
        if task.plan_status != flowstate_core::task::ApprovalStatus::Approved {
            return Err(to_error(flowstate_service::ServiceError::InvalidInput(
                format!(
                    "cannot build: plan must be approved first (current: {})",
                    task.plan_status.display_name()
                ),
            )));
        }
    }

    // Verify: build must be completed or a PR must be linked
    if action == ClaudeAction::Verify {
        let runs = state.service.list_claude_runs(&task_id).await.map_err(to_error)?;
        let has_completed_build = runs.iter().any(|r|
            r.action == ClaudeAction::Build && r.status == ClaudeRunStatus::Completed
        );
        let prs = state.service.list_task_prs(&task_id).await.map_err(to_error)?;
        if !has_completed_build && prs.is_empty() {
            return Err(to_error(flowstate_service::ServiceError::InvalidInput(
                "cannot verify: build must be completed or a PR must be linked first".to_string(),
            )));
        }
    }

    // VerifyDistill: verify artifact must exist
    if action == ClaudeAction::VerifyDistill
        && task.verify_status == flowstate_core::task::ApprovalStatus::None
    {
        return Err(to_error(flowstate_service::ServiceError::InvalidInput(
            "cannot distill verification: verification artifact must exist first".to_string(),
        )));
    }

    let create = CreateClaudeRun {
        task_id: task_id.clone(),
        action,
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
    state.runners.lock().unwrap().insert(
        runner_id.clone(),
        RunnerInfo {
            runner_id: runner_id.clone(),
            last_seen: Utc::now(),
        },
    );

    let result = state.db.claim_next_claude_run().map_err(|e| {
        to_error(flowstate_service::ServiceError::Internal(e.to_string()))
    })?;

    match result {
        Some(run) => {
            // Record which runner claimed this run
            let _ = state.db.set_claude_run_runner(&run.id, &runner_id);
            Ok((StatusCode::OK, Json(json!(run))))
        }
        None => Ok((StatusCode::NO_CONTENT, Json(json!(null)))),
    }
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
