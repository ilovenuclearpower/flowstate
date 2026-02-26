use chrono::{DateTime, Utc};

use flowstate_core::claude_run::{ClaudeAction, ClaudeRun, ClaudeRunStatus, CreateClaudeRun};

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct ClaudeRunRow {
    id: String,
    task_id: String,
    action: String,
    status: String,
    error_message: Option<String>,
    exit_code: Option<i32>,
    pr_url: Option<String>,
    pr_number: Option<i64>,
    branch_name: Option<String>,
    progress_message: Option<String>,
    runner_id: Option<String>,
    started_at: DateTime<Utc>,
    finished_at: Option<DateTime<Utc>>,
    required_capability: Option<String>,
}

impl From<ClaudeRunRow> for ClaudeRun {
    fn from(r: ClaudeRunRow) -> Self {
        ClaudeRun {
            id: r.id,
            task_id: r.task_id,
            action: ClaudeAction::parse_str(&r.action).unwrap_or(ClaudeAction::Design),
            status: ClaudeRunStatus::parse_str(&r.status).unwrap_or(ClaudeRunStatus::Queued),
            error_message: r.error_message,
            exit_code: r.exit_code,
            pr_url: r.pr_url,
            pr_number: r.pr_number,
            branch_name: r.branch_name,
            progress_message: r.progress_message,
            runner_id: r.runner_id,
            started_at: r.started_at,
            finished_at: r.finished_at,
            required_capability: r.required_capability,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_claude_run(
        &self,
        input: &CreateClaudeRun,
    ) -> Result<ClaudeRun, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO claude_runs (id, task_id, action, status, started_at, required_capability)
             VALUES ($1, $2, $3, 'queued', $4, $5)",
        )
        .bind(&id)
        .bind(&input.task_id)
        .bind(input.action.as_str())
        .bind(now)
        .bind(&input.required_capability)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, ClaudeRunRow>("SELECT * FROM claude_runs WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_get_claude_run(&self, id: &str) -> Result<ClaudeRun, DbError> {
        let row = sqlx::query_as::<_, ClaudeRunRow>("SELECT * FROM claude_runs WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| pg_not_found(&format!("claude_run {id}")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_claude_runs_for_task(
        &self,
        task_id: &str,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        let rows = sqlx::query_as::<_, ClaudeRunRow>(
            "SELECT * FROM claude_runs WHERE task_id = $1 ORDER BY started_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_update_claude_run_status(
        &self,
        id: &str,
        status: ClaudeRunStatus,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, DbError> {
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

        sqlx::query(
            "UPDATE claude_runs SET status = $1, error_message = $2, exit_code = $3, finished_at = $4
             WHERE id = $5",
        )
        .bind(status.as_str())
        .bind(error_message)
        .bind(exit_code)
        .bind(finished)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        self.pg_get_claude_run(id).await
    }

    /// Atomically claim the oldest queued run, setting it to Running.
    /// Uses FOR UPDATE SKIP LOCKED for Postgres concurrency safety.
    /// If `capabilities` is non-empty, only claim runs whose `required_capability`
    /// is NULL or matches one of the given values.
    pub(crate) async fn pg_claim_next_claude_run(
        &self,
        capabilities: &[&str],
    ) -> Result<Option<ClaudeRun>, DbError> {
        let mut tx = self.pool.begin().await.map_err(pg_err)?;
        let now = Utc::now();

        let maybe_row = if capabilities.is_empty() {
            sqlx::query_as::<_, ClaudeRunRow>(
                "SELECT * FROM claude_runs WHERE status = 'queued' ORDER BY started_at ASC LIMIT 1 FOR UPDATE SKIP LOCKED",
            )
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_err)?
        } else {
            // Convert capabilities to a Vec<String> for sqlx binding
            let caps: Vec<String> = capabilities.iter().map(|s| s.to_string()).collect();
            sqlx::query_as::<_, ClaudeRunRow>(
                "SELECT * FROM claude_runs WHERE status = 'queued' AND (required_capability IS NULL OR required_capability = ANY($1)) ORDER BY started_at ASC LIMIT 1 FOR UPDATE SKIP LOCKED",
            )
            .bind(&caps)
            .fetch_optional(&mut *tx)
            .await
            .map_err(pg_err)?
        };

        let row = match maybe_row {
            Some(r) => r,
            None => {
                tx.commit().await.map_err(pg_err)?;
                return Ok(None);
            }
        };

        sqlx::query("UPDATE claude_runs SET status = 'running', started_at = $1 WHERE id = $2")
            .bind(now)
            .bind(&row.id)
            .execute(&mut *tx)
            .await
            .map_err(pg_err)?;

        let updated = sqlx::query_as::<_, ClaudeRunRow>("SELECT * FROM claude_runs WHERE id = $1")
            .bind(&row.id)
            .fetch_one(&mut *tx)
            .await
            .map_err(pg_err)?;

        tx.commit().await.map_err(pg_err)?;

        Ok(Some(updated.into()))
    }

    pub(crate) async fn pg_update_claude_run_progress(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE claude_runs SET progress_message = $1 WHERE id = $2")
            .bind(message)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(())
    }

    pub(crate) async fn pg_update_claude_run_pr(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, DbError> {
        sqlx::query(
            "UPDATE claude_runs SET pr_url = $1, pr_number = $2, branch_name = $3 WHERE id = $4",
        )
        .bind(pr_url)
        .bind(pr_number)
        .bind(branch_name)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        self.pg_get_claude_run(id).await
    }

    pub(crate) async fn pg_find_stale_running_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        let rows = sqlx::query_as::<_, ClaudeRunRow>(
            "SELECT * FROM claude_runs WHERE status = 'running' AND started_at < $1",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_find_stale_salvaging_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        let rows = sqlx::query_as::<_, ClaudeRunRow>(
            "SELECT * FROM claude_runs WHERE status = 'salvaging' AND started_at < $1",
        )
        .bind(older_than)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_timeout_claude_run(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<Option<ClaudeRun>, DbError> {
        let now = Utc::now();

        let result = sqlx::query(
            "UPDATE claude_runs SET status = 'timed_out', error_message = $1, finished_at = $2
             WHERE id = $3 AND status IN ('running', 'salvaging')",
        )
        .bind(error_message)
        .bind(now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Ok(None);
        }

        let run = self.pg_get_claude_run(id).await?;
        Ok(Some(run))
    }

    pub(crate) async fn pg_count_queued_runs(&self) -> Result<i64, DbError> {
        let (count,): (i64,) =
            sqlx::query_as("SELECT COUNT(*) FROM claude_runs WHERE status = 'queued'")
                .fetch_one(&self.pool)
                .await
                .map_err(pg_err)?;
        Ok(count)
    }

    pub(crate) async fn pg_set_claude_run_runner(
        &self,
        id: &str,
        runner_id: &str,
    ) -> Result<(), DbError> {
        sqlx::query("UPDATE claude_runs SET runner_id = $1 WHERE id = $2")
            .bind(runner_id)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(())
    }
}
