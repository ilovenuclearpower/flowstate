use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::attachment::Attachment;

use super::super::{SqliteDatabase, SqliteResultExt};
use crate::DbError;

fn row_to_attachment(row: &Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        filename: row.get("filename")?,
        store_key: row.get("store_key")?,
        size_bytes: row.get("size_bytes")?,
        created_at: row.get("created_at")?,
    })
}

impl SqliteDatabase {
    pub fn create_attachment_sync(
        &self,
        task_id: &str,
        filename: &str,
        store_key: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO attachments (id, task_id, filename, store_key, size_bytes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, task_id, filename, store_key, size_bytes, now],
            )
            .to_db()?;
            conn.query_row(
                "SELECT * FROM attachments WHERE id = ?1",
                params![id],
                row_to_attachment,
            )
            .map_err(|e| DbError::Internal(e.to_string()))
        })
    }

    pub fn list_attachments_sync(&self, task_id: &str) -> Result<Vec<Attachment>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM attachments WHERE task_id = ?1 ORDER BY created_at DESC")
                .to_db()?;
            let attachments = stmt
                .query_map(params![task_id], row_to_attachment)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(attachments)
        })
    }

    pub fn get_attachment_sync(&self, id: &str) -> Result<Attachment, DbError> {
        self.with_conn(|conn| {
            conn.query_row(
                "SELECT * FROM attachments WHERE id = ?1",
                params![id],
                row_to_attachment,
            )
            .map_err(|e| match e {
                rusqlite::Error::QueryReturnedNoRows => {
                    DbError::NotFound(format!("attachment {id}"))
                }
                other => DbError::Internal(other.to_string()),
            })
        })
    }

    pub fn delete_attachment_sync(&self, id: &str) -> Result<Attachment, DbError> {
        self.with_conn(|conn| {
            let attachment = conn
                .query_row(
                    "SELECT * FROM attachments WHERE id = ?1",
                    params![id],
                    row_to_attachment,
                )
                .map_err(|e| match e {
                    rusqlite::Error::QueryReturnedNoRows => {
                        DbError::NotFound(format!("attachment {id}"))
                    }
                    other => DbError::Internal(other.to_string()),
                })?;
            conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])
                .to_db()?;
            Ok(attachment)
        })
    }
}
