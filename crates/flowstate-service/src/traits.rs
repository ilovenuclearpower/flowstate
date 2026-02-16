use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
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
/// `HttpService` wraps an HTTP client.
pub trait TaskService {
    // -- Projects --
    fn list_projects(&self) -> Result<Vec<Project>, ServiceError>;
    fn get_project(&self, id: &str) -> Result<Project, ServiceError>;
    fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError>;
    fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError>;
    fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, ServiceError>;
    fn delete_project(&self, id: &str) -> Result<(), ServiceError>;

    // -- Tasks --
    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError>;
    fn get_task(&self, id: &str) -> Result<Task, ServiceError>;
    fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError>;
    fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError>;
    fn delete_task(&self, id: &str) -> Result<(), ServiceError>;
    fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError>;
    fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError>;

    // -- Task Links --
    fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError>;
    fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError>;
    fn delete_task_link(&self, id: &str) -> Result<(), ServiceError>;

    // -- Claude Runs --
    fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError>;
    fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError>;
    fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError>;

    // -- Attachments --
    fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError>;
}
