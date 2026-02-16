use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::attachment::Attachment;

use crate::{Db, DbError};

fn row_to_attachment(row: &Row) -> rusqlite::Result<Attachment> {
    Ok(Attachment {
        id: row.get("id")?,
        task_id: row.get("task_id")?,
        filename: row.get("filename")?,
        disk_path: row.get("disk_path")?,
        size_bytes: row.get("size_bytes")?,
        created_at: row.get("created_at")?,
    })
}

impl Db {
    pub fn create_attachment(
        &self,
        task_id: &str,
        filename: &str,
        disk_path: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now();
            conn.execute(
                "INSERT INTO attachments (id, task_id, filename, disk_path, size_bytes, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                params![id, task_id, filename, disk_path, size_bytes, now],
            )?;
            conn.query_row(
                "SELECT * FROM attachments WHERE id = ?1",
                params![id],
                row_to_attachment,
            )
            .map_err(DbError::from)
        })
    }

    pub fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare(
                "SELECT * FROM attachments WHERE task_id = ?1 ORDER BY created_at DESC",
            )?;
            let attachments = stmt
                .query_map(params![task_id], row_to_attachment)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(attachments)
        })
    }

    pub fn get_attachment(&self, id: &str) -> Result<Attachment, DbError> {
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
                other => DbError::Sqlite(other),
            })
        })
    }

    pub fn delete_attachment(&self, id: &str) -> Result<Attachment, DbError> {
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
                    other => DbError::Sqlite(other),
                })?;
            conn.execute("DELETE FROM attachments WHERE id = ?1", params![id])?;
            Ok(attachment)
        })
    }
}
