use chrono::{DateTime, Utc};

use flowstate_core::task_pr::{CreateTaskPr, TaskPr};

use crate::DbError;
use super::super::{pg_err, PostgresDatabase};

#[derive(sqlx::FromRow)]
struct TaskPrRow {
    id: String,
    task_id: String,
    claude_run_id: Option<String>,
    pr_url: String,
    pr_number: i64,
    branch_name: String,
    created_at: DateTime<Utc>,
}

impl From<TaskPrRow> for TaskPr {
    fn from(r: TaskPrRow) -> Self {
        TaskPr {
            id: r.id,
            task_id: r.task_id,
            claude_run_id: r.claude_run_id,
            pr_url: r.pr_url,
            pr_number: r.pr_number,
            branch_name: r.branch_name,
            created_at: r.created_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_task_pr(
        &self,
        input: &CreateTaskPr,
    ) -> Result<TaskPr, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        // Use ON CONFLICT DO NOTHING for idempotent insert (unique on pr_url)
        sqlx::query(
            "INSERT INTO task_prs (id, task_id, claude_run_id, pr_url, pr_number, branch_name, created_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)
             ON CONFLICT (pr_url) DO NOTHING",
        )
        .bind(&id)
        .bind(&input.task_id)
        .bind(&input.claude_run_id)
        .bind(&input.pr_url)
        .bind(input.pr_number)
        .bind(&input.branch_name)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        // Fetch by pr_url (which is unique) to handle both insert and conflict cases
        let row = sqlx::query_as::<_, TaskPrRow>(
            "SELECT * FROM task_prs WHERE pr_url = $1",
        )
        .bind(&input.pr_url)
        .fetch_one(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_task_prs(
        &self,
        task_id: &str,
    ) -> Result<Vec<TaskPr>, DbError> {
        let rows = sqlx::query_as::<_, TaskPrRow>(
            "SELECT * FROM task_prs WHERE task_id = $1 ORDER BY created_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }
}
