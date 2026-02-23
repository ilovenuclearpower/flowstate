use chrono::{DateTime, Utc};

use flowstate_core::project::{CreateProject, Project, ProviderType, UpdateProject};

use crate::DbError;
use super::super::{pg_err, pg_not_found, PostgresDatabase};

#[derive(sqlx::FromRow)]
struct ProjectRow {
    id: String,
    name: String,
    slug: String,
    description: String,
    repo_url: String,
    repo_token: Option<String>,
    provider_type: Option<String>,
    skip_tls_verify: bool,
    created_at: DateTime<Utc>,
    updated_at: DateTime<Utc>,
}

impl From<ProjectRow> for Project {
    fn from(r: ProjectRow) -> Self {
        Project {
            id: r.id,
            name: r.name,
            slug: r.slug,
            description: r.description,
            repo_url: r.repo_url,
            repo_token: r.repo_token,
            provider_type: r.provider_type.as_deref().and_then(ProviderType::parse_str),
            skip_tls_verify: r.skip_tls_verify,
            created_at: r.created_at,
            updated_at: r.updated_at,
        }
    }
}

impl PostgresDatabase {
    pub(crate) async fn pg_create_project(
        &self,
        input: &CreateProject,
    ) -> Result<Project, DbError> {
        let id = uuid::Uuid::new_v4().to_string();
        let now = Utc::now();

        sqlx::query(
            "INSERT INTO projects (id, name, slug, description, repo_url, created_at, updated_at)
             VALUES ($1, $2, $3, $4, $5, $6, $7)",
        )
        .bind(&id)
        .bind(&input.name)
        .bind(&input.slug)
        .bind(&input.description)
        .bind(&input.repo_url)
        .bind(now)
        .bind(now)
        .execute(&self.pool)
        .await
        .map_err(pg_err)?;

        let row = sqlx::query_as::<_, ProjectRow>(
            "SELECT * FROM projects WHERE id = $1",
        )
        .bind(&id)
        .fetch_one(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(row.into())
    }

    pub(crate) async fn pg_get_project(&self, id: &str) -> Result<Project, DbError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "SELECT * FROM projects WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| pg_not_found(&format!("project {id}")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_get_project_by_slug(
        &self,
        slug: &str,
    ) -> Result<Project, DbError> {
        let row = sqlx::query_as::<_, ProjectRow>(
            "SELECT * FROM projects WHERE slug = $1",
        )
        .bind(slug)
        .fetch_optional(&self.pool)
        .await
        .map_err(pg_err)?
        .ok_or_else(|| pg_not_found(&format!("project with slug '{slug}'")))?;

        Ok(row.into())
    }

    pub(crate) async fn pg_list_projects(&self) -> Result<Vec<Project>, DbError> {
        let rows = sqlx::query_as::<_, ProjectRow>(
            "SELECT * FROM projects ORDER BY name",
        )
        .fetch_all(&self.pool)
        .await
        .map_err(pg_err)?;

        Ok(rows.into_iter().map(|r| r.into()).collect())
    }

    pub(crate) async fn pg_update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, DbError> {
        let mut sets = Vec::new();
        let mut param_idx = 1usize;
        // We'll build a dynamic query using string formatting for SET clauses
        // and collect bind values in order.

        // String params and a separate bool param tracked by position.
        struct Param {
            value: String,
        }
        let mut params: Vec<Param> = Vec::new();

        // Track the bool param position and value separately since sqlx
        // needs typed binds.
        let mut bool_bind: Option<(usize, bool)> = None;

        if let Some(ref name) = update.name {
            sets.push(format!("name = ${param_idx}"));
            params.push(Param { value: name.clone() });
            param_idx += 1;
        }
        if let Some(ref description) = update.description {
            sets.push(format!("description = ${param_idx}"));
            params.push(Param { value: description.clone() });
            param_idx += 1;
        }
        if let Some(ref repo_url) = update.repo_url {
            sets.push(format!("repo_url = ${param_idx}"));
            params.push(Param { value: repo_url.clone() });
            param_idx += 1;
        }
        if let Some(ref repo_token) = update.repo_token {
            sets.push(format!("repo_token = ${param_idx}"));
            params.push(Param { value: repo_token.clone() });
            param_idx += 1;
        }
        if let Some(ref provider_type) = update.provider_type {
            sets.push(format!("provider_type = ${param_idx}"));
            params.push(Param { value: provider_type.as_str().to_string() });
            param_idx += 1;
        }
        if let Some(skip_tls_verify) = update.skip_tls_verify {
            sets.push(format!("skip_tls_verify = ${param_idx}"));
            bool_bind = Some((param_idx, skip_tls_verify));
            param_idx += 1;
        }

        if sets.is_empty() {
            return self.pg_get_project(id).await;
        }

        sets.push(format!("updated_at = ${param_idx}"));
        let now = Utc::now();
        param_idx += 1;

        let id_param = param_idx;

        let sql = format!(
            "UPDATE projects SET {} WHERE id = ${id_param}",
            sets.join(", ")
        );

        let mut query = sqlx::query(&sql);
        for p in &params {
            query = query.bind(&p.value);
        }
        if let Some((_, val)) = bool_bind {
            query = query.bind(val);
        }
        query = query.bind(now);
        query = query.bind(id);

        let result = query.execute(&self.pool).await.map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("project {id}")));
        }

        self.pg_get_project(id).await
    }

    pub(crate) async fn pg_delete_project(&self, id: &str) -> Result<(), DbError> {
        let result = sqlx::query("DELETE FROM projects WHERE id = $1")
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(pg_err)?;

        if result.rows_affected() == 0 {
            return Err(pg_not_found(&format!("project {id}")));
        }

        Ok(())
    }
}
