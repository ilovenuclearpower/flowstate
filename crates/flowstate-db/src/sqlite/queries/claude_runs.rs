use chrono::{DateTime, Utc};
use rusqlite::{params, Row};

use flowstate_core::claude_run::{ClaudeAction, ClaudeRun, ClaudeRunStatus, CreateClaudeRun};

use super::super::{SqliteDatabase, SqliteResultExt};
use crate::DbError;

fn row_to_claude_run(row: &Row) -> rusqlite::Result<ClaudeRun> {
    let action_str: String = row.get("action")?;
    let status_str: String = row.get("status")?;
    Ok(ClaudeRun {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        action: ClaudeAction::parse_str(&action_str).unwrap_or(ClaudeAction::Design),
        status: ClaudeRunStatus::parse_str(&status_str).unwrap_or(ClaudeRunStatus::Queued),
        error_message: row.get("error_message")?,
        exit_code: row.get("exit_code")?,
        pr_url: row.get("pr_url")?,
        pr_number: row.get("pr_number")?,
        branch_name: row.get("branch_name")?,
        progress_message: row.get("progress_message")?,
        runner_id: row.get("runner_id").unwrap_or(None),
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
        required_capability: row.get("required_capability").unwrap_or(None),
    })
}

impl SqliteDatabase {
    pub fn create_claude_run_sync(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO claude_runs (id, task_id, action, status, started_at, required_capability)
                 VALUES (?1, ?2, ?3, 'queued', ?4, ?5)",
                params![id, input.task_id, input.action.as_str(), now, input.required_capability],
            )
            .to_db()?;
            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn get_claude_run_sync(&self, id: &str) -> Result<ClaudeRun, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("claude_run {id}"))
                }
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn list_claude_runs_for_task_sync(&self, task_id: &str) -> Result<Vec<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM claude_runs WHERE task_id = ?1 ORDER BY started_at DESC")
                .to_db()?;
            let runs = stmt
                .query_map(params![task_id], row_to_claude_run)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(runs)
        })
    }

    pub fn update_claude_run_status_sync(
        &self,
        id: &str,
        status: ClaudeRunStatus,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now();
            let finished = if matches!(
                status,
                ClaudeRunStatus::Completed
                    | ClaudeRunStatus::Failed
                    | ClaudeRunStatus::Cancelled
                    | ClaudeRunStatus::TimedOut
            ) {
                Some(now)
            } else {
                None
            };

            conn.execute(
                "UPDATE claude_runs SET status = ?1, error_message = ?2, exit_code = ?3, finished_at = ?4
                 WHERE id = ?5",
                params![status.as_str(), error_message, exit_code, finished, id],
            )
            .to_db()?;

            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    /// Atomically claim the oldest queued run, setting it to Running.
    /// If `capabilities` is non-empty, only claim runs whose `required_capability`
    /// is NULL or matches one of the given values.
    /// Returns None if no matching queued runs exist.
    pub fn claim_next_claude_run_sync(
        &self,
        capabilities: &[&str],
    ) -> Result<Option<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now();

            if capabilities.is_empty() {
                // No capability filter — claim any queued run
                let result = conn.query_row(
                    "UPDATE claude_runs
                     SET status = 'running', started_at = ?1
                     WHERE id = (
                         SELECT id FROM claude_runs
                         WHERE status = 'queued'
                         ORDER BY started_at ASC
                         LIMIT 1
                     )
                     RETURNING *",
                    params![now],
                    row_to_claude_run,
                );

                match result {
                    Ok(run) => Ok(Some(run)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(DbError::Internal(e.to_string())),
                }
            } else {
                // Build dynamic IN clause for capabilities
                let placeholders: Vec<String> = capabilities
                    .iter()
                    .enumerate()
                    .map(|(i, _)| format!("?{}", i + 2))
                    .collect();
                let in_clause = placeholders.join(", ");

                let sql = format!(
                    "UPDATE claude_runs
                     SET status = 'running', started_at = ?1
                     WHERE id = (
                         SELECT id FROM claude_runs
                         WHERE status = 'queued'
                           AND (required_capability IS NULL OR required_capability IN ({in_clause}))
                         ORDER BY started_at ASC
                         LIMIT 1
                     )
                     RETURNING *"
                );

                let mut stmt = conn.prepare(&sql).to_db()?;
                let mut param_values: Vec<Box<dyn rusqlite::types::ToSql>> = Vec::new();
                param_values.push(Box::new(now.to_rfc3339()));
                for cap in capabilities {
                    param_values.push(Box::new(cap.to_string()));
                }
                let param_refs: Vec<&dyn rusqlite::types::ToSql> =
                    param_values.iter().map(|b| b.as_ref()).collect();

                let result = stmt.query_row(param_refs.as_slice(), row_to_claude_run);

                match result {
                    Ok(run) => Ok(Some(run)),
                    Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                    Err(e) => Err(DbError::Internal(e.to_string())),
                }
            }
        })
    }

    /// Update the progress message on a claude run.
    pub fn update_claude_run_progress_sync(&self, id: &str, message: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE claude_runs SET progress_message = ?1 WHERE id = ?2",
                params![message, id],
            )
            .to_db()?;
            Ok(())
        })
    }

    /// Update PR info on a claude run.
    pub fn update_claude_run_pr_sync(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE claude_runs SET pr_url = ?1, pr_number = ?2, branch_name = ?3
                 WHERE id = ?4",
                params![pr_url, pr_number, branch_name, id],
            )
            .to_db()?;

            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    /// Find all runs stuck in Running status beyond the given threshold.
    /// Used by the server-side watchdog.
    pub fn find_stale_running_runs_sync(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM claude_runs WHERE status = 'running' AND started_at < ?1")
                .to_db()?;
            let runs = stmt
                .query_map(params![older_than], row_to_claude_run)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(runs)
        })
    }

    /// Find all runs stuck in Salvaging status beyond the given threshold.
    /// Used by the server-side watchdog.
    pub fn find_stale_salvaging_runs_sync(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM claude_runs WHERE status = 'salvaging' AND started_at < ?1")
                .to_db()?;
            let runs = stmt
                .query_map(params![older_than], row_to_claude_run)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(runs)
        })
    }

    /// Atomically transition a run from Running or Salvaging to TimedOut.
    /// Returns Ok(None) if the run was not in those statuses (race-safe).
    pub fn timeout_claude_run_sync(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<Option<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now();
            let affected = conn
                .execute(
                    "UPDATE claude_runs SET status = 'timed_out', error_message = ?1, finished_at = ?2
                     WHERE id = ?3 AND status IN ('running', 'salvaging')",
                    params![error_message, now, id],
                )
                .to_db()?;

            if affected == 0 {
                return Ok(None);
            }

            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map(Some)
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    /// Count runs in queued status.
    pub fn count_queued_runs_sync(&self) -> Result<i64, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM claude_runs WHERE status = 'queued'",
                [],
                |row| row.get(0),
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    /// Set runner_id on a claude run (at claim time).
    pub fn set_claude_run_runner_sync(&self, id: &str, runner_id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            conn.execute(
                "UPDATE claude_runs SET runner_id = ?1 WHERE id = ?2",
                params![runner_id, id],
            )
            .to_db()?;
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;
    use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus, CreateClaudeRun};
    use flowstate_core::project::CreateProject;
    use flowstate_core::task::{CreateTask, Priority, Status};

    fn setup() -> (Db, String) {
        let db = Db::open_in_memory().unwrap();
        let project = db
            .create_project_sync(&CreateProject {
                name: "Test".into(),
                slug: "test".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .unwrap();
        let task = db
            .create_task_sync(&CreateTask {
                project_id: project.id,
                title: "Test task".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .unwrap();
        (db, task.id)
    }

    #[test]
    fn test_claude_run_crud() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Design,
                required_capability: None,
            })
            .unwrap();
        assert_eq!(run.status, ClaudeRunStatus::Queued);
        assert_eq!(run.action, ClaudeAction::Design);
        assert!(run.pr_url.is_none());
        assert!(run.pr_number.is_none());
        assert!(run.branch_name.is_none());
        assert!(run.runner_id.is_none());

        let fetched = db.get_claude_run_sync(&run.id).unwrap();
        assert_eq!(fetched.id, run.id);

        let updated = db
            .update_claude_run_status_sync(&run.id, ClaudeRunStatus::Completed, None, Some(0))
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Completed);
        assert!(updated.finished_at.is_some());

        let runs = db.list_claude_runs_for_task_sync(&task_id).unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn test_claim_next_claude_run() {
        let (db, task_id) = setup();

        // No queued runs -> None
        assert!(db.claim_next_claude_run_sync(&[]).unwrap().is_none());

        // Create two runs
        let run1 = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Design,
                required_capability: None,
            })
            .unwrap();
        let _run2 = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Plan,
                required_capability: None,
            })
            .unwrap();

        // Claim should get the oldest (run1)
        let claimed = db.claim_next_claude_run_sync(&[]).unwrap().unwrap();
        assert_eq!(claimed.id, run1.id);
        assert_eq!(claimed.status, ClaudeRunStatus::Running);

        // Next claim gets run2
        let claimed2 = db.claim_next_claude_run_sync(&[]).unwrap().unwrap();
        assert_eq!(claimed2.action, ClaudeAction::Plan);

        // No more queued
        assert!(db.claim_next_claude_run_sync(&[]).unwrap().is_none());
    }

    #[test]
    fn test_update_claude_run_pr() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id,
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();

        let updated = db
            .update_claude_run_pr_sync(
                &run.id,
                Some("https://github.com/org/repo/pull/42"),
                Some(42),
                Some("flowstate/my-feature"),
            )
            .unwrap();

        assert_eq!(
            updated.pr_url.as_deref(),
            Some("https://github.com/org/repo/pull/42")
        );
        assert_eq!(updated.pr_number, Some(42));
        assert_eq!(updated.branch_name.as_deref(), Some("flowstate/my-feature"));
    }

    #[test]
    fn test_new_status_variants() {
        let (db, task_id) = setup();

        // Test TimedOut status
        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();
        let updated = db
            .update_claude_run_status_sync(
                &run.id,
                ClaudeRunStatus::TimedOut,
                Some("timed out"),
                None,
            )
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::TimedOut);
        assert!(updated.finished_at.is_some());
        assert_eq!(updated.error_message.as_deref(), Some("timed out"));

        // Test Salvaging status (non-terminal, no finished_at)
        let run2 = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();
        let _ = db.claim_next_claude_run_sync(&[]).unwrap(); // claim run2 to set it Running
        let updated2 = db
            .update_claude_run_status_sync(&run2.id, ClaudeRunStatus::Salvaging, None, None)
            .unwrap();
        assert_eq!(updated2.status, ClaudeRunStatus::Salvaging);
        assert!(updated2.finished_at.is_none());
    }

    #[test]
    fn test_find_stale_running_runs() {
        let (db, task_id) = setup();

        // Create and claim a run (sets it to Running)
        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();
        let _claimed = db.claim_next_claude_run_sync(&[]).unwrap().unwrap();

        // With a threshold in the future, the run should be returned
        let future = chrono::Utc::now() + chrono::Duration::hours(1);
        let stale = db.find_stale_running_runs_sync(future).unwrap();
        assert_eq!(stale.len(), 1);
        assert_eq!(stale[0].id, run.id);

        // With a threshold in the past, no runs should be returned
        let past = chrono::Utc::now() - chrono::Duration::hours(1);
        let stale = db.find_stale_running_runs_sync(past).unwrap();
        assert!(stale.is_empty());

        // Completed runs should not be returned
        let _ =
            db.update_claude_run_status_sync(&run.id, ClaudeRunStatus::Completed, None, Some(0));
        let stale = db.find_stale_running_runs_sync(future).unwrap();
        assert!(stale.is_empty());
    }

    #[test]
    fn test_timeout_claude_run() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();

        // Claim to set Running
        let _claimed = db.claim_next_claude_run_sync(&[]).unwrap().unwrap();

        // Timeout should transition Running -> TimedOut
        let result = db
            .timeout_claude_run_sync(&run.id, "watchdog timeout")
            .unwrap();
        assert!(result.is_some());
        let timed_out = result.unwrap();
        assert_eq!(timed_out.status, ClaudeRunStatus::TimedOut);
        assert_eq!(timed_out.error_message.as_deref(), Some("watchdog timeout"));
        assert!(timed_out.finished_at.is_some());

        // Calling again on a TimedOut run should return None (already terminal)
        let result2 = db
            .timeout_claude_run_sync(&run.id, "second timeout")
            .unwrap();
        assert!(result2.is_none());
    }

    #[test]
    fn test_count_queued_runs_empty() {
        let (db, _task_id) = setup();
        let count = db.count_queued_runs_sync().unwrap();
        assert_eq!(count, 0);
    }

    #[test]
    fn test_count_queued_runs_with_runs() {
        let (db, task_id) = setup();

        // Create 3 queued runs
        for _ in 0..3 {
            db.create_claude_run_sync(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Research,
                required_capability: None,
            })
            .unwrap();
        }
        assert_eq!(db.count_queued_runs_sync().unwrap(), 3);

        // Claim one (transitions to running)
        db.claim_next_claude_run_sync(&[]).unwrap();
        assert_eq!(db.count_queued_runs_sync().unwrap(), 2);

        // Complete the claimed run — still 2 queued
        let runs = db.list_claude_runs_for_task_sync(&task_id).unwrap();
        let running = runs.iter().find(|r| r.status == ClaudeRunStatus::Running).unwrap();
        db.update_claude_run_status_sync(&running.id, ClaudeRunStatus::Completed, None, Some(0))
            .unwrap();
        assert_eq!(db.count_queued_runs_sync().unwrap(), 2);
    }

    #[test]
    fn test_set_claude_run_runner() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run_sync(&CreateClaudeRun {
                task_id,
                action: ClaudeAction::Build,
                required_capability: None,
            })
            .unwrap();
        assert!(run.runner_id.is_none());

        db.set_claude_run_runner_sync(&run.id, "runner-1").unwrap();

        let fetched = db.get_claude_run_sync(&run.id).unwrap();
        assert_eq!(fetched.runner_id.as_deref(), Some("runner-1"));
    }
}
