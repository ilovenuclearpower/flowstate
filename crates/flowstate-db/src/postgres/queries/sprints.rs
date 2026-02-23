use chrono::{DateTime, Utc};

use flowstate_core::sprint::{CreateSprint, Sprint, SprintStatus, UpdateSprint};

use super::super::{pg_err, pg_not_found, PostgresDatabase};
use crate::DbError;

#[derive(sqlx::FromRow)]
struct SprintRow {
    id: String,
    project_id: String,
    name: String,
    goal: String,
    starts_at: Option<DateTime<Utc>>,
    ends_at: Option<DateTime<Utc>>,
    status: String,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<SprintRow> for Sprint {
    fn from(r: SprintRow) -> Self {
        Sprint {
            id: r.id,
            project_id: r.project_id,
            name: r.name,
            goal: r.goal,
            starts_at: r.starts_at,
            ends_at: r.ends_at,
            status: SprintStatus::parse_str(&r.status).unwrap_or(SprintStatus::Planned),
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_sprint(&self, input: &CreateSprint) -> Result<Sprint, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO sprints (id, project_id, name, goal, starts_at, ends_at, status, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(&id)
        .bind(&input.project_id)
        .bind(&input.name)
        .bind(&input.goal)
        .bind(input.starts_at)
        .bind(input.ends_at)
        .bind("planned")
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, SprintRow>("SELECT * FROM sprints WHERE id = $1")
            .bind(&id)
            .fetch_one(&self.pool)
            .await
            .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_get_sprint(&self, id: &str) -> Result<Sprint, DbError> {
        let row = sqlx::query_as::<_, SprintRow>("SELECT * FROM sprints WHERE id = $1")
            .bind(id)
            .fetch_optional(&self.pool)
            .await
            .map_err(pg_err)?
            .ok_or_else(|| pg_not_found(&format!("sprint {id}")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, DbError> {
        let rows = sqlx::query_as::<_, SprintRow>(
            "SELECT * FROM sprints WHERE project_id = $1 ORDER BY created_at DESC",
        )
        .bind(project_id)
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_update_sprint(
        &self,
        id: &str,
        update: &UpdateSprint,
    ) -> Result<Sprint, DbError> {
        if update.name.is_none()
            && update.goal.is_none()
            && update.status.is_none()
            && update.starts_at.is_none()
            && update.ends_at.is_none()
        {
            return self.pg_get_sprint(id).await;
        }

        let now = Utc::now();

        let mut sets = Vec::new();
        let mut param_idx = 1usize;

        enum BindValue {
            Str(String),
            OptDateTime(Option<DateTime<Utc>>),
            DateTime(DateTime<Utc>),
        }
        let mut binds: Vec<BindValue> = Vec::new();

        if let Some(ref name) = update.name {
            sets.push(format!("name = ${param_idx}"));
            binds.push(BindValue::Str(name.clone()));
            param_idx += 1;
        }
        if let Some(ref goal) = update.goal {
            sets.push(format!("goal = ${param_idx}"));
            binds.push(BindValue::Str(goal.clone()));
            param_idx += 1;
        }
        if let Some(ref status) = update.status {
            sets.push(format!("status = ${param_idx}"));
            binds.push(BindValue::Str(status.as_str().to_string()));
            param_idx += 1;
        }
        if let Some(ref starts_at) = update.starts_at {
            sets.push(format!("starts_at = ${param_idx}"));
            binds.push(BindValue::OptDateTime(*starts_at));
            param_idx += 1;
        }
        if let Some(ref ends_at) = update.ends_at {
            sets.push(format!("ends_at = ${param_idx}"));
            binds.push(BindValue::OptDateTime(*ends_at));
            param_idx += 1;
        }

        sets.push(format!("updated_at = ${param_idx}"));
        binds.push(BindValue::DateTime(now));
        param_idx += 1;

        let id_param = param_idx;
        binds.push(BindValue::Str(id.to_string()));

        let sql = format!(
            "UPDATE sprints SET {} WHERE id = ${id_param}",
            sets.join(", ")
        );

        let mut query = sqlx::query(&sql);
        for bind in &binds {
            match bind {
                BindValue::Str(s) => {
                    query = query.bind(s);
                }
                BindValue::OptDateTime(dt) => {
                    query = query.bind(dt);
                }
                BindValue::DateTime(dt) => {
                    query = query.bind(dt);
                }
            }
        }

        let result = query.execute(&self.pool).await.map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("sprint {id}")));
        }

        self.pg_get_sprint(id).await
    }

    pub(crate) async fn pg_delete_sprint(&self, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM sprints WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("sprint {id}")));
        }

        Ok(())
    }
}
