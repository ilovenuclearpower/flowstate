#[cfg(feature = "sqlite")]
pub mod sqlite;

#[cfg(feature = "postgres")]
pub mod postgres;

use std::path::PathBuf;
use std::sync::Arc;

use async_trait::async_trait;
use chrono::{DateTime, Utc};
use thiserror::Error;

use flowstate_core::api_key::ApiKey;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, ClaudeRunStatus, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};

#[derive(Debug, Error)]
pub enum DbError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("database error: {0}")]
    Internal(String),

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
}

#[async_trait]
pub trait Database: Send + Sync {
    // -- Projects (6 methods) --
    async fn create_project(&self, input: &CreateProject) -> Result<Project, DbError>;
    async fn get_project(&self, id: &str) -> Result<Project, DbError>;
    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, DbError>;
    async fn list_projects(&self) -> Result<Vec<Project>, DbError>;
    async fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, DbError>;
    async fn delete_project(&self, id: &str) -> Result<(), DbError>;

    // -- Tasks (7 methods) --
    async fn create_task(&self, input: &CreateTask) -> Result<Task, DbError>;
    async fn get_task(&self, id: &str) -> Result<Task, DbError>;
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, DbError>;
    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, DbError>;
    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, DbError>;
    async fn delete_task(&self, id: &str) -> Result<(), DbError>;
    async fn count_tasks_by_status(&self, project_id: &str) -> Result<Vec<(String, i64)>, DbError>;

    // -- Claude Runs (11 methods) --
    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, DbError>;
    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, DbError>;
    async fn list_claude_runs_for_task(&self, task_id: &str) -> Result<Vec<ClaudeRun>, DbError>;
    async fn update_claude_run_status(
        &self,
        id: &str,
        status: ClaudeRunStatus,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, DbError>;
    async fn claim_next_claude_run(&self) -> Result<Option<ClaudeRun>, DbError>;
    async fn update_claude_run_progress(&self, id: &str, message: &str) -> Result<(), DbError>;
    async fn update_claude_run_pr(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, DbError>;
    async fn find_stale_running_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError>;
    async fn find_stale_salvaging_runs(
        &self,
        older_than: DateTime<Utc>,
    ) -> Result<Vec<ClaudeRun>, DbError>;
    async fn timeout_claude_run(
        &self,
        id: &str,
        error_message: &str,
    ) -> Result<Option<ClaudeRun>, DbError>;
    async fn set_claude_run_runner(&self, id: &str, runner_id: &str) -> Result<(), DbError>;

    // -- Sprints (5 methods) --
    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, DbError>;
    async fn get_sprint(&self, id: &str) -> Result<Sprint, DbError>;
    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, DbError>;
    async fn update_sprint(&self, id: &str, update: &UpdateSprint) -> Result<Sprint, DbError>;
    async fn delete_sprint(&self, id: &str) -> Result<(), DbError>;

    // -- Task Links (3 methods) --
    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, DbError>;
    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, DbError>;
    async fn delete_task_link(&self, id: &str) -> Result<(), DbError>;

    // -- Task PRs (2 methods) --
    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, DbError>;
    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, DbError>;

    // -- Attachments (4 methods) --
    async fn create_attachment(
        &self,
        task_id: &str,
        filename: &str,
        store_key: &str,
        size_bytes: i64,
    ) -> Result<Attachment, DbError>;
    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, DbError>;
    async fn get_attachment(&self, id: &str) -> Result<Attachment, DbError>;
    async fn delete_attachment(&self, id: &str) -> Result<Attachment, DbError>;

    // -- API Keys (6 methods) --
    async fn insert_api_key(&self, name: &str, key_hash: &str) -> Result<ApiKey, DbError>;
    async fn find_api_key_by_hash(&self, key_hash: &str) -> Result<Option<ApiKey>, DbError>;
    async fn touch_api_key(&self, id: &str) -> Result<(), DbError>;
    async fn has_api_keys(&self) -> Result<bool, DbError>;
    async fn list_api_keys(&self) -> Result<Vec<ApiKey>, DbError>;
    async fn delete_api_key(&self, id: &str) -> Result<(), DbError>;
}

// -- Configuration --

/// Configuration for database backend selection.
pub struct DbConfig {
    /// Backend type: "sqlite" or "postgres"
    pub backend: String,
    /// Postgres connection URL (required when backend = "postgres")
    pub database_url: Option<String>,
    /// SQLite file path (optional; defaults to ~/.local/share/flowstate/flowstate.db)
    pub sqlite_path: Option<String>,
}

impl DbConfig {
    /// Build from environment variables.
    ///
    /// - `FLOWSTATE_DB_BACKEND`: "sqlite" (default) or "postgres"
    /// - `FLOWSTATE_DATABASE_URL` or `DATABASE_URL`: Postgres connection URL
    /// - `FLOWSTATE_SQLITE_PATH`: Override default SQLite file path
    pub fn from_env() -> Self {
        Self {
            backend: std::env::var("FLOWSTATE_DB_BACKEND")
                .unwrap_or_else(|_| "sqlite".into()),
            database_url: std::env::var("FLOWSTATE_DATABASE_URL")
                .or_else(|_| std::env::var("DATABASE_URL"))
                .ok(),
            sqlite_path: std::env::var("FLOWSTATE_SQLITE_PATH").ok(),
        }
    }
}

/// Create a database backend from configuration.
pub async fn open_database(config: &DbConfig) -> Result<Arc<dyn Database>, DbError> {
    match config.backend.as_str() {
        "sqlite" => {
            #[cfg(feature = "sqlite")]
            {
                let db = sqlite::SqliteDatabase::open(config)?;
                Ok(Arc::new(db))
            }
            #[cfg(not(feature = "sqlite"))]
            {
                Err(DbError::Internal(
                    "SQLite backend requested but the 'sqlite' feature is not enabled".into(),
                ))
            }
        }
        "postgres" => {
            #[cfg(feature = "postgres")]
            {
                let url = config.database_url.as_deref().ok_or_else(|| {
                    DbError::Internal(
                        "Postgres backend requires FLOWSTATE_DATABASE_URL or DATABASE_URL".into(),
                    )
                })?;
                let db = postgres::PostgresDatabase::connect(url).await?;
                Ok(Arc::new(db))
            }
            #[cfg(not(feature = "postgres"))]
            {
                Err(DbError::Internal(
                    "Postgres backend requested but the 'postgres' feature is not enabled".into(),
                ))
            }
        }
        other => Err(DbError::Internal(format!("unknown database backend: {other}"))),
    }
}

// Re-export for convenience
#[cfg(feature = "sqlite")]
pub use sqlite::SqliteDatabase;

// Backwards-compat alias — will be removed after consumers migrate to Arc<dyn Database>
#[cfg(feature = "sqlite")]
pub type Db = SqliteDatabase;

// -- File path helpers --
// These are filesystem-specific utilities used by flowstate-server for managing
// workspace directories and run output files. They stay as standalone functions.

pub fn data_dir() -> PathBuf {
    dirs_default_data_dir().join("flowstate")
}

pub fn task_dir(task_id: &str) -> PathBuf {
    data_dir().join("tasks").join(task_id)
}

pub fn task_spec_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("specification.md")
}

pub fn task_plan_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("plan.md")
}

pub fn task_research_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("research.md")
}

pub fn task_verification_path(task_id: &str) -> PathBuf {
    task_dir(task_id).join("verification.md")
}

pub fn task_attachments_dir(task_id: &str) -> PathBuf {
    task_dir(task_id).join("attachments")
}

pub fn claude_run_dir(run_id: &str) -> PathBuf {
    data_dir().join("claude_runs").join(run_id)
}

pub fn workspace_dir(project_id: &str) -> PathBuf {
    data_dir().join("workspaces").join(project_id)
}

fn dirs_default_data_dir() -> PathBuf {
    if let Ok(xdg) = std::env::var("XDG_DATA_HOME") {
        PathBuf::from(xdg)
    } else if let Some(home) = std::env::var_os("HOME") {
        PathBuf::from(home).join(".local/share")
    } else {
        PathBuf::from(".")
    }
}
