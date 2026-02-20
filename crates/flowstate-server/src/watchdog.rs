use std::time::Duration;

use chrono::Utc;
use flowstate_db::Db;
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
pub async fn run_watchdog(db: Db, scan_interval_secs: u64) {
    let mut ticker = tokio::time::interval(Duration::from_secs(scan_interval_secs));
    loop {
        ticker.tick().await;
        if let Err(e) = check_stale_runs(&db) {
            error!("watchdog error: {e}");
        }
    }
}

fn check_stale_runs(db: &Db) -> Result<(), Box<dyn std::error::Error>> {
    // Hard timeout for runs stuck in Running: 90 minutes
    let running_timeout = chrono::Duration::minutes(90);
    let running_threshold = Utc::now() - running_timeout;
    let stale_running = db.find_stale_running_runs(running_threshold)?;

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
        )?;
    }

    // Hard timeout for runs stuck in Salvaging: 30 minutes
    let salvage_timeout = chrono::Duration::minutes(30);
    let salvage_threshold = Utc::now() - salvage_timeout;
    let stale_salvaging = db.find_stale_salvaging_runs(salvage_threshold)?;

    for run in stale_salvaging {
        warn!(
            "watchdog: timing out stale salvage run {} (action={}, started_at={})",
            run.id, run.action, run.started_at
        );
        db.timeout_claude_run(
            &run.id,
            "server watchdog: salvage agent timed out",
        )?;
    }

    Ok(())
}
