use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_db::Db;

use crate::{ServiceError, TaskService};

/// Local implementation backed by direct SQLite access.
pub struct LocalService {
    db: Db,
}

impl LocalService {
    pub fn new(db: Db) -> Self {
        Self { db }
    }
}

impl From<flowstate_db::DbError> for ServiceError {
    fn from(e: flowstate_db::DbError) -> Self {
        match e {
            flowstate_db::DbError::NotFound(msg) => ServiceError::NotFound(msg),
            other => ServiceError::Internal(other.to_string()),
        }
    }
}

impl TaskService for LocalService {
    fn list_projects(&self) -> Result<Vec<Project>, ServiceError> {
        Ok(self.db.list_projects()?)
    }

    fn get_project(&self, id: &str) -> Result<Project, ServiceError> {
        Ok(self.db.get_project(id)?)
    }

    fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError> {
        Ok(self.db.get_project_by_slug(slug)?)
    }

    fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError> {
        Ok(self.db.create_project(input)?)
    }

    fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, ServiceError> {
        Ok(self.db.update_project(id, update)?)
    }

    fn delete_project(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_project(id)?)
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError> {
        Ok(self.db.list_tasks(filter)?)
    }

    fn get_task(&self, id: &str) -> Result<Task, ServiceError> {
        Ok(self.db.get_task(id)?)
    }

    fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError> {
        Ok(self.db.create_task(input)?)
    }

    fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError> {
        Ok(self.db.update_task(id, update)?)
    }

    fn delete_task(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_task(id)?)
    }

    fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError> {
        Ok(self.db.count_tasks_by_status(project_id)?)
    }

    fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError> {
        Ok(self.db.list_child_tasks(parent_id)?)
    }

    fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError> {
        Ok(self.db.create_task_link(input)?)
    }

    fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError> {
        Ok(self.db.list_task_links(task_id)?)
    }

    fn delete_task_link(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_task_link(id)?)
    }

    fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError> {
        Ok(self.db.create_claude_run(input)?)
    }

    fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError> {
        Ok(self.db.get_claude_run(id)?)
    }

    fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError> {
        Ok(self.db.list_claude_runs_for_task(task_id)?)
    }

    fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError> {
        Ok(self.db.list_attachments(task_id)?)
    }
}
