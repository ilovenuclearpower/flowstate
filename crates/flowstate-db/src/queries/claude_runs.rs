use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::claude_run::{ClaudeAction, ClaudeRun, ClaudeRunStatus, CreateClaudeRun};

use crate::{Db, DbError};

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
        started_at: row.get("started_at")?,
        finished_at: row.get("finished_at")?,
    })
}

impl Db {
    pub fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO claude_runs (id, task_id, action, status, started_at)
                 VALUES (?1, ?2, ?3, 'queued', ?4)",
                params![id, input.task_id, input.action.as_str(), now],
            )?;
            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(DbError::from)
        })
    }

    pub fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, DbError> {
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
                other => DbError::Sqlite(other),
            })
        })
    }

    pub fn list_claude_runs_for_task(&self, task_id: &str) -> Result<Vec<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM claude_runs WHERE task_id = ?1 ORDER BY started_at DESC",
            )?;
            let runs = stmt
                .query_map(params![task_id], row_to_claude_run)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(runs)
        })
    }

    pub fn update_claude_run_status(
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
                ClaudeRunStatus::Completed | ClaudeRunStatus::Failed | ClaudeRunStatus::Cancelled
            ) {
                Some(now)
            } else {
                None
            };

            conn.execute(
                "UPDATE claude_runs SET status = ?1, error_message = ?2, exit_code = ?3, finished_at = ?4
                 WHERE id = ?5",
                params![status.as_str(), error_message, exit_code, finished, id],
            )?;

            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(DbError::from)
        })
    }

    /// Atomically claim the oldest queued run, setting it to Running.
    /// Returns None if no queued runs exist.
    pub fn claim_next_claude_run(&self) -> Result<Option<ClaudeRun>, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now();
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
                Err(e) => Err(DbError::Sqlite(e)),
            }
        })
    }

    /// Update PR info on a claude run.
    pub fn update_claude_run_pr(
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
            )?;

            conn.query_row(
                "SELECT * FROM claude_runs WHERE id = ?1",
                params![id],
                row_to_claude_run,
            )
            .map_err(DbError::from)
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
            .create_project(&CreateProject {
                name: "Test".into(),
                slug: "test".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .unwrap();
        let task = db
            .create_task(&CreateTask {
                project_id: project.id,
                title: "Test task".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .unwrap();
        (db, task.id)
    }

    #[test]
    fn test_claude_run_crud() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Design,
            })
            .unwrap();
        assert_eq!(run.status, ClaudeRunStatus::Queued);
        assert_eq!(run.action, ClaudeAction::Design);
        assert!(run.pr_url.is_none());
        assert!(run.pr_number.is_none());
        assert!(run.branch_name.is_none());

        let fetched = db.get_claude_run(&run.id).unwrap();
        assert_eq!(fetched.id, run.id);

        let updated = db
            .update_claude_run_status(&run.id, ClaudeRunStatus::Completed, None, Some(0))
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Completed);
        assert!(updated.finished_at.is_some());

        let runs = db.list_claude_runs_for_task(&task_id).unwrap();
        assert_eq!(runs.len(), 1);
    }

    #[test]
    fn test_claim_next_claude_run() {
        let (db, task_id) = setup();

        // No queued runs → None
        assert!(db.claim_next_claude_run().unwrap().is_none());

        // Create two runs
        let run1 = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Design,
            })
            .unwrap();
        let _run2 = db
            .create_claude_run(&CreateClaudeRun {
                task_id: task_id.clone(),
                action: ClaudeAction::Plan,
            })
            .unwrap();

        // Claim should get the oldest (run1)
        let claimed = db.claim_next_claude_run().unwrap().unwrap();
        assert_eq!(claimed.id, run1.id);
        assert_eq!(claimed.status, ClaudeRunStatus::Running);

        // Next claim gets run2
        let claimed2 = db.claim_next_claude_run().unwrap().unwrap();
        assert_eq!(claimed2.action, ClaudeAction::Plan);

        // No more queued
        assert!(db.claim_next_claude_run().unwrap().is_none());
    }

    #[test]
    fn test_update_claude_run_pr() {
        let (db, task_id) = setup();

        let run = db
            .create_claude_run(&CreateClaudeRun {
                task_id,
                action: ClaudeAction::Build,
            })
            .unwrap();

        let updated = db
            .update_claude_run_pr(
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
}
