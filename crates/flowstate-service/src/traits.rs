use async_trait::async_trait;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};
use thiserror::Error;

#[derive(Debug, Error)]
pub enum ServiceError {
    #[error("not found: {0}")]
    NotFound(String),

    #[error("invalid input: {0}")]
    InvalidInput(String),

    #[error("internal error: {0}")]
    Internal(String),
}

/// Abstraction over task tracking operations.
///
/// The TUI and MCP server program against this trait.
/// `LocalService` wraps a direct SQLite connection.
/// `HttpService` wraps an async HTTP client.
#[async_trait]
pub trait TaskService: Send + Sync {
    // -- Projects --
    async fn list_projects(&self) -> Result<Vec<Project>, ServiceError>;
    async fn get_project(&self, id: &str) -> Result<Project, ServiceError>;
    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError>;
    async fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError>;
    async fn update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, ServiceError>;
    async fn delete_project(&self, id: &str) -> Result<(), ServiceError>;

    // -- Tasks --
    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError>;
    async fn get_task(&self, id: &str) -> Result<Task, ServiceError>;
    async fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError>;
    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError>;
    async fn delete_task(&self, id: &str) -> Result<(), ServiceError>;
    async fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError>;
    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError>;

    // -- Sprints --
    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, ServiceError>;
    async fn get_sprint(&self, id: &str) -> Result<Sprint, ServiceError>;
    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, ServiceError>;
    async fn update_sprint(
        &self,
        id: &str,
        update: &UpdateSprint,
    ) -> Result<Sprint, ServiceError>;
    async fn delete_sprint(&self, id: &str) -> Result<(), ServiceError>;

    // -- Task Links --
    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError>;
    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError>;
    async fn delete_task_link(&self, id: &str) -> Result<(), ServiceError>;

    // -- Task PRs --
    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, ServiceError>;
    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, ServiceError>;

    // -- Claude Runs --
    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError>;
    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError>;
    async fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError>;

    // -- Attachments --
    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError>;
}
