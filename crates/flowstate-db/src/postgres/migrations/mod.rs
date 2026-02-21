use sqlx::PgPool;

use crate::DbError;

pub async fn run(pool: &PgPool) -> Result<(), DbError> {
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

    let current: i64 =
        sqlx::query_scalar("SELECT COALESCE(MAX(version), 0) FROM schema_version")
            .fetch_one(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;

    if current < 1 {
        sqlx::query(include_str!("sql/V1__initial.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    if current < 2 {
        sqlx::query(include_str!("sql/V2__add_required_capability.sql"))
            .execute(pool)
            .await
            .map_err(|e| DbError::Internal(e.to_string()))?;
    }

    Ok(())
}
