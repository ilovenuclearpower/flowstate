use chrono::{DateTime, Utc};

use flowstate_core::task_link::{CreateTaskLink, LinkType, TaskLink};

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct TaskLinkRow {
    id: String,
    source_task_id: String,
    target_task_id: String,
    link_type: String,
    created_at: DateTime<Utc>,
}

impl From<TaskLinkRow> for TaskLink {
    fn from(r: TaskLinkRow) -> Self {
        TaskLink {
            id: r.id,
            source_task_id: r.source_task_id,
            target_task_id: r.target_task_id,
            link_type: LinkType::parse_str(&r.link_type).unwrap_or(LinkType::RelatesTo),
            created_at: r.created_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_task_link(
        &self,
        input: &CreateTaskLink,
    ) -> Result<TaskLink, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO task_links (id, source_task_id, target_task_id, link_type, created_at)
             VALUES ($1, $2, $3, $4, $5)",
        )
        .bind(&id)
        .bind(&input.source_task_id)
        .bind(&input.target_task_id)
        .bind(input.link_type.as_str())
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, TaskLinkRow>("SELECT * FROM task_links WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, DbError> {
        let rows = sqlx::query_as::<_, TaskLinkRow>(
            "SELECT * FROM task_links
             WHERE source_task_id = $1 OR target_task_id = $1
             ORDER BY created_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_delete_task_link(&self, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM task_links WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("task_link {id}")));
        }

        Ok(())
    }
}
