pub(crate) mod migrations;
pub mod queries;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use sqlx::postgres::PgPoolOptions;
use sqlx::PgPool;

use flowstate_core::api_key::ApiKey;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, ClaudeRunStatus, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};

use crate::{Database, DbError};

/// Map a sqlx::Error into a DbError::Internal.
pub(crate) fn pg_err(e: sqlx::Error) -> DbError {
    DbError::Internal(e.to_string())
}

/// Create a DbError::NotFound with the given entity description.
pub(crate) fn pg_not_found(entity: &str) -> DbError {
    DbError::NotFound(entity.to_string())
}

#[derive(Clone)]
pub struct PostgresDatabase {
    pub(crate) pool: PgPool,
}

impl PostgresDatabase {
    /// Connect to a Postgres database and run migrations.
    pub async fn connect(url: &str) -> Result<Self, DbError> {
        let pool = PgPoolOptions::new()
            .max_connections(10)
            .connect(url)
            .await
            .map_err(pg_err)?;

        let db = Self { pool };
        migrations::run(&db.pool).await?;
        Ok(db)
    }
}

#[async_trait]
impl Database for PostgresDatabase {
    // -- Projects --
    async fn create_project(&self, input: &CreateProject) -> Result<Project, DbError> {
        self.pg_create_project(input).await
    }
    async fn get_project(&self, id: &str) -> Result<Project, DbError> {
        self.pg_get_project(id).await
    }
    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, DbError> {
        self.pg_get_project_by_slug(slug).await
    }
    async fn list_projects(&self) -> Result<Vec<Project>, DbError> {
        self.pg_list_projects().await
    }
    async fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, DbError> {
        self.pg_update_project(id, update).await
    }
    async fn delete_project(&self, id: &str) -> Result<(), DbError> {
        self.pg_delete_project(id).await
    }

    // -- Tasks --
    async fn create_task(&self, input: &CreateTask) -> Result<Task, DbError> {
        self.pg_create_task(input).await
    }
    async fn get_task(&self, id: &str) -> Result<Task, DbError> {
        self.pg_get_task(id).await
    }
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, DbError> {
        self.pg_list_tasks(filter).await
    }
    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, DbError> {
        self.pg_list_child_tasks(parent_id).await
    }
    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, DbError> {
        self.pg_update_task(id, update).await
    }
    async fn delete_task(&self, id: &str) -> Result<(), DbError> {
        self.pg_delete_task(id).await
    }
    async fn count_tasks_by_status(&self, project_id: &str) -> Result<Vec<(String, i64)>, DbError> {
        self.pg_count_tasks_by_status(project_id).await
    }

    // -- Claude Runs --
    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, DbError> {
        self.pg_create_claude_run(input).await
    }
    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, DbError> {
        self.pg_get_claude_run(id).await
    }
    async fn list_claude_runs_for_task(&self, task_id: &str) -> Result<Vec<ClaudeRun>, DbError> {
        self.pg_list_claude_runs_for_task(task_id).await
    }
    async fn update_claude_run_status(
        &self,
        id: &str,
        status: ClaudeRunStatus,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, DbError> {
        self.pg_update_claude_run_status(id, status, error_message, exit_code)
            .await
    }
    async fn claim_next_claude_run(
        &self,
        capabilities: &[&str],
    ) -> Result<Option<ClaudeRun>, DbError> {
        self.pg_claim_next_claude_run(capabilities).await
    }
    async fn update_claude_run_progress(&self, id: &str, message: &str) -> Result<(), DbError> {
        self.pg_update_claude_run_progress(id, message).await
    }
    async fn update_claude_run_pr(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, DbError> {
        self.pg_update_claude_run_pr(id, pr_url, pr_number, branch_name)
            .await
    }
    async fn find_stale_running_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        self.pg_find_stale_running_runs(older_than).await
    }
    async fn find_stale_salvaging_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError> {
        self.pg_find_stale_salvaging_runs(older_than).await
    }
    async fn timeout_claude_run(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<Option<ClaudeRun>, DbError> {
        self.pg_timeout_claude_run(id, error_message).await
    }
    async fn set_claude_run_runner(&self, id: &str, runner_id: &str) -> Result<(), DbError> {
        self.pg_set_claude_run_runner(id, runner_id).await
    }
    async fn count_queued_runs(&self) -> Result<i64, DbError> {
        self.pg_count_queued_runs().await
    }

    // -- Sprints --
    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, DbError> {
        self.pg_create_sprint(input).await
    }
    async fn get_sprint(&self, id: &str) -> Result<Sprint, DbError> {
        self.pg_get_sprint(id).await
    }
    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, DbError> {
        self.pg_list_sprints(project_id).await
    }
    async fn update_sprint(&self, id: &str, update: &UpdateSprint) -> Result<Sprint, DbError> {
        self.pg_update_sprint(id, update).await
    }
    async fn delete_sprint(&self, id: &str) -> Result<(), DbError> {
        self.pg_delete_sprint(id).await
    }

    // -- Task Links --
    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, DbError> {
        self.pg_create_task_link(input).await
    }
    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, DbError> {
        self.pg_list_task_links(task_id).await
    }
    async fn delete_task_link(&self, id: &str) -> Result<(), DbError> {
        self.pg_delete_task_link(id).await
    }

    // -- Task PRs --
    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, DbError> {
        self.pg_create_task_pr(input).await
    }
    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, DbError> {
        self.pg_list_task_prs(task_id).await
    }

    // -- Attachments --
    async fn create_attachment(
        &self,
        task_id: &str,
        filename: &str,
        store_key: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError> {
        self.pg_create_attachment(task_id, filename, store_key, size_bytes)
            .await
    }
    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, DbError> {
        self.pg_list_attachments(task_id).await
    }
    async fn get_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        self.pg_get_attachment(id).await
    }
    async fn delete_attachment(&self, id: &str) -> Result<Attachment, DbError> {
        self.pg_delete_attachment(id).await
    }

    // -- API Keys --
    async fn insert_api_key(&self, name: &str, key_hash: &str) -> Result<ApiKey, DbError> {
        self.pg_insert_api_key(name, key_hash).await
    }
    async fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, DbError> {
        self.pg_find_api_key_by_hash(key_hash).await
    }
    async fn touch_api_key(&self, id: &str) -> Result<(), DbError> {
        self.pg_touch_api_key(id).await
    }
    async fn has_api_keys(&self) -> Result<bool, DbError> {
        self.pg_has_api_keys().await
    }
    async fn list_api_keys(&self) -> Result<Vec<ApiKey>, DbError> {
        self.pg_list_api_keys().await
    }
    async fn delete_api_key(&self, id: &str) -> Result<(), DbError> {
        self.pg_delete_api_key(id).await
    }
}
