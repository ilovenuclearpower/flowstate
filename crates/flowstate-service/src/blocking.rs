use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};
use tokio::runtime::Runtime;

use crate::{HttpService, ServiceError, TaskService};

/// Blocking wrapper around the async `HttpService`.
///
/// Creates an internal tokio runtime and uses `block_on()` for each call.
/// Designed for sync callers like the TUI.
pub struct BlockingHttpService {
    inner: HttpService,
    rt: Runtime,
}

impl BlockingHttpService {
    pub fn new(base_url: &str) -> Self {
        Self {
            inner: HttpService::new(base_url),
            rt: Runtime::new().expect("failed to create tokio runtime"),
        }
    }

    pub fn with_api_key(base_url: &str, key: String) -> Self {
        Self {
            inner: HttpService::with_api_key(base_url, key),
            rt: Runtime::new().expect("failed to create tokio runtime"),
        }
    }

    pub fn health_check(&self) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.health_check())
    }

    // -- Trait method delegates --

    pub fn list_projects(&self) -> Result<Vec<Project>, ServiceError> {
        self.rt.block_on(self.inner.list_projects())
    }

    pub fn get_project(&self, id: &str) -> Result<Project, ServiceError> {
        self.rt.block_on(self.inner.get_project(id))
    }

    pub fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError> {
        self.rt.block_on(self.inner.get_project_by_slug(slug))
    }

    pub fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError> {
        self.rt.block_on(self.inner.create_project(input))
    }

    pub fn update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, ServiceError> {
        self.rt.block_on(self.inner.update_project(id, update))
    }

    pub fn delete_project(&self, id: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.delete_project(id))
    }

    pub fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError> {
        self.rt.block_on(self.inner.list_tasks(filter))
    }

    pub fn get_task(&self, id: &str) -> Result<Task, ServiceError> {
        self.rt.block_on(self.inner.get_task(id))
    }

    pub fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError> {
        self.rt.block_on(self.inner.create_task(input))
    }

    pub fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError> {
        self.rt.block_on(self.inner.update_task(id, update))
    }

    pub fn delete_task(&self, id: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.delete_task(id))
    }

    pub fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError> {
        self.rt
            .block_on(self.inner.count_tasks_by_status(project_id))
    }

    pub fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError> {
        self.rt.block_on(self.inner.list_child_tasks(parent_id))
    }

    pub fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, ServiceError> {
        self.rt.block_on(self.inner.create_sprint(input))
    }

    pub fn get_sprint(&self, id: &str) -> Result<Sprint, ServiceError> {
        self.rt.block_on(self.inner.get_sprint(id))
    }

    pub fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, ServiceError> {
        self.rt.block_on(self.inner.list_sprints(project_id))
    }

    pub fn update_sprint(
        &self,
        id: &str,
        update: &UpdateSprint,
    ) -> Result<Sprint, ServiceError> {
        self.rt.block_on(self.inner.update_sprint(id, update))
    }

    pub fn delete_sprint(&self, id: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.delete_sprint(id))
    }

    pub fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError> {
        self.rt.block_on(self.inner.create_task_link(input))
    }

    pub fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError> {
        self.rt.block_on(self.inner.list_task_links(task_id))
    }

    pub fn delete_task_link(&self, id: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.delete_task_link(id))
    }

    pub fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, ServiceError> {
        self.rt.block_on(self.inner.create_task_pr(input))
    }

    pub fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, ServiceError> {
        self.rt.block_on(self.inner.list_task_prs(task_id))
    }

    pub fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError> {
        self.rt.block_on(self.inner.create_claude_run(input))
    }

    pub fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError> {
        self.rt.block_on(self.inner.get_claude_run(id))
    }

    pub fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError> {
        self.rt.block_on(self.inner.list_claude_runs(task_id))
    }

    pub fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError> {
        self.rt.block_on(self.inner.list_attachments(task_id))
    }

    // -- Convenience methods --

    pub fn trigger_claude_run(
        &self,
        task_id: &str,
        action: &str,
    ) -> Result<ClaudeRun, ServiceError> {
        self.rt
            .block_on(self.inner.trigger_claude_run(task_id, action))
    }

    pub fn get_claude_run_output(&self, run_id: &str) -> Result<String, ServiceError> {
        self.rt
            .block_on(self.inner.get_claude_run_output(run_id))
    }

    pub fn read_task_spec(&self, task_id: &str) -> Result<String, ServiceError> {
        self.rt.block_on(self.inner.read_task_spec(task_id))
    }

    pub fn write_task_spec(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.rt
            .block_on(self.inner.write_task_spec(task_id, content))
    }

    pub fn read_task_plan(&self, task_id: &str) -> Result<String, ServiceError> {
        self.rt.block_on(self.inner.read_task_plan(task_id))
    }

    pub fn write_task_plan(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.rt
            .block_on(self.inner.write_task_plan(task_id, content))
    }

    pub fn read_task_research(&self, task_id: &str) -> Result<String, ServiceError> {
        self.rt.block_on(self.inner.read_task_research(task_id))
    }

    pub fn write_task_research(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.write_task_research(task_id, content))
    }

    pub fn read_task_verification(&self, task_id: &str) -> Result<String, ServiceError> {
        self.rt.block_on(self.inner.read_task_verification(task_id))
    }

    pub fn write_task_verification(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.write_task_verification(task_id, content))
    }

    pub fn set_repo_token(&self, project_id: &str, token: &str) -> Result<(), ServiceError> {
        self.rt.block_on(self.inner.set_repo_token(project_id, token))
    }

    pub fn get_repo_token(&self, project_id: &str) -> Result<String, ServiceError> {
        self.rt.block_on(self.inner.get_repo_token(project_id))
    }

    pub fn system_status(&self) -> Result<crate::SystemStatus, ServiceError> {
        self.rt.block_on(self.inner.system_status())
    }
}
