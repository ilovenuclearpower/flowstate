use chrono::{DateTime, Utc};

use flowstate_core::attachment::Attachment;

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct AttachmentRow {
    id: String,
    task_id: String,
    filename: String,
    store_key: String,
    size_bytes: i64,
    created_at: DateTime<Utc>,
}

impl From<AttachmentRow> for Attachment {
    fn from(r: AttachmentRow) -> Self {
        Attachment {
            id: r.id,
            task_id: r.task_id,
            filename: r.filename,
            store_key: r.store_key,
            size_bytes: r.size_bytes,
            created_at: r.created_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_attachment(
        &self,
        task_id: &str,
        filename: &str,
        store_key: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO attachments (id, task_id, filename, store_key, size_bytes, created_at)
             VALUES ($1, $2, $3, $4, $5, $6)",
        )
        .bind(&id)
        .bind(task_id)
        .bind(filename)
        .bind(store_key)
        .bind(size_bytes)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, AttachmentRow>("SELECT * FROM attachments WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_attachments(
        &self,
        task_id: &str,
    ) -> Result<Vec<Attachment>, DbError> {
        let rows = sqlx::query_as::<_, AttachmentRow>(
            "SELECT * FROM attachments WHERE task_id = $1 ORDER BY created_at DESC",
        )
        .bind(task_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_get_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        let row = sqlx::query_as::<_, AttachmentRow>("SELECT * FROM attachments WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| pg_not_found(&format!("attachment {id}")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_delete_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        // Fetch before deleting so we can return the deleted attachment
        let attachment = self.pg_get_attachment(id).await?;

        sqlx::query("DELETE FROM attachments WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(attachment)
    }
}
