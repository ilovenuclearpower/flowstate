use std::sync::Arc;
use std::time::Duration;

use chrono::Utc;
use flowstate_db::Database;
use tracing::{error, warn};

/// Background task that detects and transitions stuck runs.
///
/// Runs periodically and looks for ClaudeRuns in Running or Salvaging status
/// whose started_at is older than the hard timeout.
///
/// Hard timeout defaults:
/// - Running: 90 minutes (1.5x the runner's default 60 min build timeout)
/// - Salvaging: 30 minutes
///
/// The server timeout should always be LONGER than the runner timeout,
/// since the runner handles its own timeout first. The server watchdog
/// is defense-in-depth for when the runner crashes.
pub async fn run_watchdog(db: Arc<dyn Database>, scan_interval_secs: u64) {
    let mut ticker = tokio::time::interval(Duration::from_secs(scan_interval_secs));
    loop {
        ticker.tick().await;
        if let Err(e) = check_stale_runs(&*db).await {
            error!("watchdog error: {e}");
        }
    }
}

async fn check_stale_runs(db: &dyn Database) -> Result<(), Box<dyn std::error::Error>> {
    // Hard timeout for runs stuck in Running: 90 minutes
    let running_timeout = chrono::Duration::minutes(90);
    let running_threshold = Utc::now() - running_timeout;
    let stale_running = db.find_stale_running_runs(running_threshold).await?;

    for run in stale_running {
        warn!(
            "watchdog: timing out stale run {} (action={}, started_at={})",
            run.id, run.action, run.started_at
        );
        db.timeout_claude_run(
            &run.id,
            &format!(
                "server watchdog: no runner activity for >{}min",
                running_timeout.num_minutes()
            ),
        ).await?;
    }

    // Hard timeout for runs stuck in Salvaging: 30 minutes
    let salvage_timeout = chrono::Duration::minutes(30);
    let salvage_threshold = Utc::now() - salvage_timeout;
    let stale_salvaging = db.find_stale_salvaging_runs(salvage_threshold).await?;

    for run in stale_salvaging {
        warn!(
            "watchdog: timing out stale salvage run {} (action={}, started_at={})",
            run.id, run.action, run.started_at
        );
        db.timeout_claude_run(
            &run.id,
            "server watchdog: salvage agent timed out",
        ).await?;
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[tokio::test]
    async fn check_stale_runs_empty_db() {
        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());
        // Should complete without errors on an empty database
        check_stale_runs(&*db).await.unwrap();
    }

    #[tokio::test]
    async fn check_recent_runs_untouched() {
        use flowstate_core::claude_run::{ClaudeRunStatus, CreateClaudeRun, ClaudeAction};
        use flowstate_core::project::CreateProject;
        use flowstate_core::task::{CreateTask, Status, Priority};

        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());

        let project = db
            .create_project(&CreateProject {
                name: "WD Recent".into(),
                slug: "wd-recent".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "WD Recent Task".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .await
            .unwrap();

        let run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        // Claim to set running (started_at is now)
        let _ = db.claim_next_claude_run(&[]).await.unwrap();

        // Run watchdog check — recent run should NOT be timed out
        check_stale_runs(&*db).await.unwrap();

        // Verify the run is still running
        let updated = db.get_claude_run(&run.id).await.unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Running);
    }

    #[tokio::test]
    async fn check_stale_runs_finds_no_stale() {
        use flowstate_core::claude_run::{CreateClaudeRun, ClaudeAction};
        use flowstate_core::project::CreateProject;
        use flowstate_core::task::{CreateTask, Status, Priority};

        let db = Arc::new(flowstate_db::SqliteDatabase::open_in_memory().unwrap());

        let project = db
            .create_project(&CreateProject {
                name: "WD NoStale".into(),
                slug: "wd-nostale".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        let task = db
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "WD NoStale Task".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .await
            .unwrap();

        // Create a queued run (not running, so won't be "stale")
        let _run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();

        // Run watchdog check — queued runs are not stale
        check_stale_runs(&*db).await.unwrap();
    }
}
