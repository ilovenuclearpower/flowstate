use chrono::Utc;

use flowstate_core::api_key::ApiKey;

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

/// ApiKey stores dates as plain TEXT strings (not TIMESTAMPTZ),
/// matching the core ApiKey type which uses String for created_at/last_used_at.
#[derive(sqlx::FromRow)]
struct ApiKeyRow {
    id: String,
    name: String,
    key_hash: String,
    created_at: String,
    last_used_at: Option<String>,
}

impl From<ApiKeyRow> for ApiKey {
    fn from(r: ApiKeyRow) -> Self {
        ApiKey {
            id: r.id,
            name: r.name,
            key_hash: r.key_hash,
            created_at: r.created_at,
            last_used_at: r.last_used_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_insert_api_key(
        &self,
        name: &str,
        key_hash: &str,
    ) -> Result<ApiKey, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO api_keys (id, name, key_hash, created_at) VALUES ($1, $2, $3, $4)",
        )
        .bind(&id)
        .bind(name)
        .bind(key_hash)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_find_api_key_by_hash(
        &self,
        key_hash: &str,
    ) -> Result<Option<ApiKey>, DbError> {
        let row = sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys WHERE key_hash = $1")
            .bind(key_hash)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.map(|r| r.into()))
    }

    pub(crate) async fn pg_touch_api_key(&self, id: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();

        sqlx::query("UPDATE api_keys SET last_used_at = $1 WHERE id = $2")
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(())
    }

    pub(crate) async fn pg_has_api_keys(&self) -> Result<bool, DbError> {
        let count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM api_keys")
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(count > 0)
    }

    pub(crate) async fn pg_list_api_keys(&self) -> Result<Vec<ApiKey>, DbError> {
        let rows =
            sqlx::query_as::<_, ApiKeyRow>("SELECT * FROM api_keys ORDER BY created_at DESC")
                .fetch_all(&self.pool)
                .await
                .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_delete_api_key(&self, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM api_keys WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("api_key {id}")));
        }

        Ok(())
    }
}
