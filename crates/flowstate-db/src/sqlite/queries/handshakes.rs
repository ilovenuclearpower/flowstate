use chrono::Utc;
use rusqlite::{params, Row};

use flowstate_core::handshake::RunnerHandshake;

use super::super::{SqliteDatabase, SqliteResultExt};
use crate::DbError;

fn row_to_handshake(row: &Row) -> rusqlite::Result<RunnerHandshake> {
    Ok(RunnerHandshake {
        id: row.get("id")?,
        runner_id: row.get("runner_id")?,
        hostname: row.get("hostname")?,
        status: row.get("status")?,
        api_key_id: row.get("api_key_id")?,
        created_at: row.get("created_at")?,
        updated_at: row.get("updated_at")?,
    })
}

impl SqliteDatabase {
    pub fn upsert_runner_handshake_sync(
        &self,
        runner_id: &str,
        hostname: &str,
    ) -> Result<RunnerHandshake, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            // Try to find existing
            let existing = conn.query_row(
                "SELECT * FROM runner_handshakes WHERE runner_id = ?1",
                params![runner_id],
                row_to_handshake,
            );
            match existing {
                Ok(h) => {
                    // Update hostname and updated_at
                    conn.execute(
                        "UPDATE runner_handshakes SET hostname = ?1, updated_at = ?2 WHERE runner_id = ?3",
                        params![hostname, now, runner_id],
                    )
                    .to_db()?;
                    conn.query_row(
                        "SELECT * FROM runner_handshakes WHERE id = ?1",
                        params![h.id],
                        row_to_handshake,
                    )
                    .map_err(|e| DbError::Internal(e.to_string()))
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => {
                    let id = uuid::Uuid::new_v4().to_string();
                    conn.execute(
                        "INSERT INTO runner_handshakes (id, runner_id, hostname, status, created_at, updated_at) VALUES (?1, ?2, ?3, 'pending', ?4, ?4)",
                        params![id, runner_id, hostname, now],
                    )
                    .to_db()?;
                    conn.query_row(
                        "SELECT * FROM runner_handshakes WHERE id = ?1",
                        params![id],
                        row_to_handshake,
                    )
                    .map_err(|e| DbError::Internal(e.to_string()))
                }
                Err(e) => Err(DbError::Internal(e.to_string())),
            }
        })
    }

    pub fn find_runner_handshake_by_runner_id_sync(
        &self,
        runner_id: &str,
    ) -> Result<Option<RunnerHandshake>, DbError> {
        self.with_conn(|conn| {
            let result = conn.query_row(
                "SELECT * FROM runner_handshakes WHERE runner_id = ?1",
                params![runner_id],
                row_to_handshake,
            );
            match result {
                Ok(h) => Ok(Some(h)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DbError::Internal(e.to_string())),
            }
        })
    }

    pub fn list_runner_handshakes_sync(&self) -> Result<Vec<RunnerHandshake>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn
                .prepare("SELECT * FROM runner_handshakes ORDER BY created_at DESC")
                .to_db()?;
            let handshakes = stmt
                .query_map([], row_to_handshake)
                .to_db()?
                .collect::<Result<Vec<_>, _>>()
                .to_db()?;
            Ok(handshakes)
        })
    }

    pub fn approve_runner_handshake_sync(
        &self,
        id: &str,
        api_key_id: &str,
        raw_key: &str,
    ) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            let changed = conn
                .execute(
                    "UPDATE runner_handshakes SET status = 'approved', api_key_id = ?1, raw_key = ?2, updated_at = ?3 WHERE id = ?4 AND status = 'pending'",
                    params![api_key_id, raw_key, now, id],
                )
                .to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("runner_handshake {id}")));
            }
            Ok(())
        })
    }

    pub fn reject_runner_handshake_sync(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            let changed = conn
                .execute(
                    "UPDATE runner_handshakes SET status = 'rejected', updated_at = ?1 WHERE id = ?2 AND status = 'pending'",
                    params![now, id],
                )
                .to_db()?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("runner_handshake {id}")));
            }
            Ok(())
        })
    }

    pub fn claim_runner_handshake_sync(&self, id: &str) -> Result<Option<String>, DbError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            // Get the raw key, then clear it
            let result = conn.query_row(
                "SELECT raw_key FROM runner_handshakes WHERE id = ?1 AND status = 'approved'",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            );
            match result {
                Ok(raw_key) => {
                    conn.execute(
                        "UPDATE runner_handshakes SET status = 'claimed', raw_key = NULL, updated_at = ?1 WHERE id = ?2",
                        params![now, id],
                    )
                    .to_db()?;
                    Ok(raw_key)
                }
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DbError::Internal(e.to_string())),
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;

    #[test]
    fn test_handshake_lifecycle() {
        let db = Db::open_in_memory().unwrap();

        // Upsert creates a new pending handshake
        let h = db
            .upsert_runner_handshake_sync("runner-1", "myhost")
            .unwrap();
        assert_eq!(h.runner_id, "runner-1");
        assert_eq!(h.hostname, "myhost");
        assert_eq!(h.status, "pending");

        // Upsert again updates hostname
        let h2 = db
            .upsert_runner_handshake_sync("runner-1", "newhost")
            .unwrap();
        assert_eq!(h2.id, h.id);
        assert_eq!(h2.hostname, "newhost");

        // Find by runner_id
        let found = db
            .find_runner_handshake_by_runner_id_sync("runner-1")
            .unwrap();
        assert!(found.is_some());

        let not_found = db
            .find_runner_handshake_by_runner_id_sync("nonexistent")
            .unwrap();
        assert!(not_found.is_none());

        // List
        let all = db.list_runner_handshakes_sync().unwrap();
        assert_eq!(all.len(), 1);

        // Approve
        let key = db
            .insert_api_key_sync("runner-key", "hash456", "runner")
            .unwrap();
        db.approve_runner_handshake_sync(&h.id, &key.id, "fs_rawkey123")
            .unwrap();

        let approved = db
            .find_runner_handshake_by_runner_id_sync("runner-1")
            .unwrap()
            .unwrap();
        assert_eq!(approved.status, "approved");
        assert_eq!(approved.api_key_id.as_deref(), Some(key.id.as_str()));

        // Claim
        let raw = db.claim_runner_handshake_sync(&h.id).unwrap();
        assert_eq!(raw.as_deref(), Some("fs_rawkey123"));

        let claimed = db
            .find_runner_handshake_by_runner_id_sync("runner-1")
            .unwrap()
            .unwrap();
        assert_eq!(claimed.status, "claimed");
    }

    #[test]
    fn test_reject_handshake() {
        let db = Db::open_in_memory().unwrap();

        let h = db
            .upsert_runner_handshake_sync("runner-2", "host2")
            .unwrap();
        db.reject_runner_handshake_sync(&h.id).unwrap();

        let rejected = db
            .find_runner_handshake_by_runner_id_sync("runner-2")
            .unwrap()
            .unwrap();
        assert_eq!(rejected.status, "rejected");
    }
}
