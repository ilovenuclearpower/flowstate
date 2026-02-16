use chrono::Utc;
use rusqlite::{params, Row};
use serde::Serialize;

use crate::{Db, DbError};

#[derive(Debug, Clone, Serialize)]
pub struct ApiKey {
    pub id: String,
    pub name: String,
    pub key_hash: String,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

fn row_to_api_key(row: &Row) -> rusqlite::Result<ApiKey> {
    Ok(ApiKey {
        id: row.get("id")?,
        name: row.get("name")?,
        key_hash: row.get("key_hash")?,
        created_at: row.get("created_at")?,
        last_used_at: row.get("last_used_at")?,
    })
}

impl Db {
    pub fn insert_api_key(&self, name: &str, key_hash: &str) -> Result<ApiKey, DbError> {
        self.with_conn(|conn| {
            let id = uuid::Uuid::new_v4().to_string();
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "INSERT INTO api_keys (id, name, key_hash, created_at) VALUES (?1, ?2, ?3, ?4)",
                params![id, name, key_hash, now],
            )?;
            conn.query_row(
                "SELECT * FROM api_keys WHERE id = ?1",
                params![id],
                row_to_api_key,
            )
            .map_err(DbError::from)
        })
    }

    pub fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, DbError> {
        self.with_conn(|conn| {
            let result = conn.query_row(
                "SELECT * FROM api_keys WHERE key_hash = ?1",
                params![key_hash],
                row_to_api_key,
            );
            match result {
                Ok(key) => Ok(Some(key)),
                Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
                Err(e) => Err(DbError::Sqlite(e)),
            }
        })
    }

    pub fn touch_api_key(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let now = Utc::now().to_rfc3339();
            conn.execute(
                "UPDATE api_keys SET last_used_at = ?1 WHERE id = ?2",
                params![now, id],
            )?;
            Ok(())
        })
    }

    pub fn has_api_keys(&self) -> Result<bool, DbError> {
        self.with_conn(|conn| {
            let count: i64 =
                conn.query_row("SELECT COUNT(*) FROM api_keys", [], |row| row.get(0))?;
            Ok(count > 0)
        })
    }

    pub fn list_api_keys(&self) -> Result<Vec<ApiKey>, DbError> {
        self.with_conn(|conn| {
            let mut stmt = conn.prepare("SELECT * FROM api_keys ORDER BY created_at DESC")?;
            let keys = stmt
                .query_map([], row_to_api_key)?
                .collect::<Result<Vec<_>, _>>()?;
            Ok(keys)
        })
    }

    pub fn delete_api_key(&self, id: &str) -> Result<(), DbError> {
        self.with_conn(|conn| {
            let changed = conn.execute("DELETE FROM api_keys WHERE id = ?1", params![id])?;
            if changed == 0 {
                return Err(DbError::NotFound(format!("api_key {id}")));
            }
            Ok(())
        })
    }
}

#[cfg(test)]
mod tests {
    use crate::Db;

    #[test]
    fn test_api_key_crud() {
        let db = Db::open_in_memory().unwrap();

        // Insert
        let key = db.insert_api_key("test-key", "hash123").unwrap();
        assert_eq!(key.name, "test-key");
        assert_eq!(key.key_hash, "hash123");
        assert!(key.last_used_at.is_none());

        // Find by hash
        let found = db.find_api_key_by_hash("hash123").unwrap();
        assert!(found.is_some());
        assert_eq!(found.unwrap().id, key.id);

        // Not found
        let missing = db.find_api_key_by_hash("nonexistent").unwrap();
        assert!(missing.is_none());

        // Has keys
        assert!(db.has_api_keys().unwrap());

        // Touch
        db.touch_api_key(&key.id).unwrap();
        let touched = db.find_api_key_by_hash("hash123").unwrap().unwrap();
        assert!(touched.last_used_at.is_some());

        // List
        let keys = db.list_api_keys().unwrap();
        assert_eq!(keys.len(), 1);

        // Delete
        db.delete_api_key(&key.id).unwrap();
        assert!(!db.has_api_keys().unwrap());
    }
}
