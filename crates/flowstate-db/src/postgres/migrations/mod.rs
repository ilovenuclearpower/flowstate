use sqlx::PgPool;

use crate::DbError;

/// Arbitrary but fixed key for the Postgres advisory lock that serialises
/// migration runs so concurrent connections don't race.
const MIGRATION_LOCK_KEY: i64 = 0x666C6F77_73746174; // "flowstat" as hex

pub async fn run(pool: &PgPool) -> Result<(), DbError> {
    // Acquire a session-level advisory lock so only one connection migrates
    // at a time.  `pg_advisory_lock` blocks until the lock is available and
    // is automatically released when the session/connection is returned to
    // the pool, but we release it explicitly below.
    sqlx::query("SELECT pg_advisory_lock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(pool)
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?;

    let result = run_inner(pool).await;

    // Always release the advisory lock, even on error.
    let _ = sqlx::query("SELECT pg_advisory_unlock($1)")
        .bind(MIGRATION_LOCK_KEY)
        .execute(pool)
        .await;

    result
}

async fn run_inner(pool: &PgPool) -> Result<(), DbError> {
    // Create schema_version if needed
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS schema_version (
            version    INTEGER PRIMARY KEY,
            applied_at TIMESTAMPTZ NOT NULL
        )",
    )
    .execute(pool)
    .await
    .map_err(|e| DbError::Internal(e.to_string()))?;

    let current: i32 = sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
        .fetch_one(pool)
        .await
        .map_err(|e| DbError::Internal(e.to_string()))?;

    if current < 1 {
        sqlx::raw_sql(include_str!("sql/V1__initial.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    if current < 2 {
        sqlx::raw_sql(include_str!("sql/V2__add_required_capability.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    if current < 3 {
        sqlx::raw_sql(include_str!("sql/V3__add_provider_type.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    if current < 4 {
        sqlx::raw_sql(include_str!("sql/V4__add_task_capabilities.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    Ok(())
}
