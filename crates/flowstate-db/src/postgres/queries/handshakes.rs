use chrono::Utc;

use flowstate_core::handshake::RunnerHandshake;

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct HandshakeRow {
    id: String,
    runner_id: String,
    hostname: String,
    status: String,
    api_key_id: Option<String>,
    created_at: String,
    updated_at: String,
}

impl From<HandshakeRow> for RunnerHandshake {
    fn from(r: HandshakeRow) -> Self {
        RunnerHandshake {
            id: r.id,
            runner_id: r.runner_id,
            hostname: r.hostname,
            status: r.status,
            api_key_id: r.api_key_id,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_upsert_runner_handshake(
        &self,
        runner_id: &str,
        hostname: &str,
    ) -> Result<RunnerHandshake, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now().to_rfc3339();

        sqlx::query(
            "INSERT INTO runner_handshakes (id, runner_id, hostname, status, created_at, updated_at)
             VALUES ($1, $2, $3, 'pending', $4, $4)
             ON CONFLICT (runner_id) DO UPDATE SET hostname = $3, updated_at = $4",
        )
        .bind(&id)
        .bind(runner_id)
        .bind(hostname)
        .bind(&now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, HandshakeRow>(
            "SELECT id, runner_id, hostname, status, api_key_id, created_at, updated_at FROM runner_handshakes WHERE runner_id = $1",
        )
        .bind(runner_id)
        .fetch_one(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_find_runner_handshake_by_runner_id(
        &self,
        runner_id: &str,
    ) -> Result<Option<RunnerHandshake>, DbError> {
        let row = sqlx::query_as::<_, HandshakeRow>(
            "SELECT id, runner_id, hostname, status, api_key_id, created_at, updated_at FROM runner_handshakes WHERE runner_id = $1",
        )
        .bind(runner_id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(row.map(|r| r.into()))
    }

    pub(crate) async fn pg_list_runner_handshakes(&self) -> Result<Vec<RunnerHandshake>, DbError> {
        let rows = sqlx::query_as::<_, HandshakeRow>(
            "SELECT id, runner_id, hostname, status, api_key_id, created_at, updated_at FROM runner_handshakes ORDER BY created_at DESC",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_approve_runner_handshake(
        &self,
        id: &str,
        api_key_id: &str,
        raw_key: &str,
    ) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE runner_handshakes SET status = 'approved', api_key_id = $1, raw_key = $2, updated_at = $3 WHERE id = $4 AND status = 'pending'",
        )
        .bind(api_key_id)
        .bind(raw_key)
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("runner_handshake {id}")));
        }
        Ok(())
    }

    pub(crate) async fn pg_reject_runner_handshake(&self, id: &str) -> Result<(), DbError> {
        let now = Utc::now().to_rfc3339();
        let result = sqlx::query(
            "UPDATE runner_handshakes SET status = 'rejected', updated_at = $1 WHERE id = $2 AND status = 'pending'",
        )
        .bind(&now)
        .bind(id)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("runner_handshake {id}")));
        }
        Ok(())
    }

    pub(crate) async fn pg_claim_runner_handshake(
        &self,
        id: &str,
    ) -> Result<Option<String>, DbError> {
        let now = Utc::now().to_rfc3339();

        let raw_key: Option<String> = sqlx::query_scalar(
            "SELECT raw_key FROM runner_handshakes WHERE id = $1 AND status = 'approved'",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?
        .flatten();

        if raw_key.is_some() {
            sqlx::query(
                "UPDATE runner_handshakes SET status = 'claimed', raw_key = NULL, updated_at = $1 WHERE id = $2",
            )
            .bind(&now)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;
        }

        Ok(raw_key)
    }
}
