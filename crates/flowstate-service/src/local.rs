use async_trait::async_trait;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};
use flowstate_db::Database;
use std::sync::Arc;

use crate::{ServiceError, TaskService};

/// Local implementation backed by direct SQLite access.
pub struct LocalService {
    db: Arc<dyn Database>,
}

impl LocalService {
    pub fn new(db: Arc<dyn Database>) -> Self {
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

#[async_trait]
impl TaskService for LocalService {
    async fn list_projects(&self) -> Result<Vec<Project>, ServiceError> {
        Ok(self.db.list_projects().await?)
    }

    async fn get_project(&self, id: &str) -> Result<Project, ServiceError> {
        Ok(self.db.get_project(id).await?)
    }

    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError> {
        Ok(self.db.get_project_by_slug(slug).await?)
    }

    async fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError> {
        Ok(self.db.create_project(input).await?)
    }

    async fn update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, ServiceError> {
        Ok(self.db.update_project(id, update).await?)
    }

    async fn delete_project(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_project(id).await?)
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError> {
        Ok(self.db.list_tasks(filter).await?)
    }

    async fn get_task(&self, id: &str) -> Result<Task, ServiceError> {
        Ok(self.db.get_task(id).await?)
    }

    async fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError> {
        Ok(self.db.create_task(input).await?)
    }

    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError> {
        Ok(self.db.update_task(id, update).await?)
    }

    async fn delete_task(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_task(id).await?)
    }

    async fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError> {
        Ok(self.db.count_tasks_by_status(project_id).await?)
    }

    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError> {
        Ok(self.db.list_child_tasks(parent_id).await?)
    }

    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, ServiceError> {
        Ok(self.db.create_sprint(input).await?)
    }

    async fn get_sprint(&self, id: &str) -> Result<Sprint, ServiceError> {
        Ok(self.db.get_sprint(id).await?)
    }

    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, ServiceError> {
        Ok(self.db.list_sprints(project_id).await?)
    }

    async fn update_sprint(
        &self,
        id: &str,
        update: &UpdateSprint,
    ) -> Result<Sprint, ServiceError> {
        Ok(self.db.update_sprint(id, update).await?)
    }

    async fn delete_sprint(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_sprint(id).await?)
    }

    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError> {
        Ok(self.db.create_task_link(input).await?)
    }

    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError> {
        Ok(self.db.list_task_links(task_id).await?)
    }

    async fn delete_task_link(&self, id: &str) -> Result<(), ServiceError> {
        Ok(self.db.delete_task_link(id).await?)
    }

    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, ServiceError> {
        Ok(self.db.create_task_pr(input).await?)
    }

    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, ServiceError> {
        Ok(self.db.list_task_prs(task_id).await?)
    }

    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError> {
        Ok(self.db.create_claude_run(input).await?)
    }

    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError> {
        Ok(self.db.get_claude_run(id).await?)
    }

    async fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError> {
        Ok(self.db.list_claude_runs_for_task(task_id).await?)
    }

    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError> {
        Ok(self.db.list_attachments(task_id).await?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::project::CreateProject;
    use flowstate_core::sprint::CreateSprint;
    use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
    use flowstate_db::SqliteDatabase;

    async fn make_service() -> LocalService {
        let db = Arc::new(SqliteDatabase::open_in_memory().unwrap());
        LocalService::new(db as Arc<dyn Database>)
    }

    #[test]
    fn test_db_error_not_found_maps_to_service_not_found() {
        let db_err = flowstate_db::DbError::NotFound("task-1".into());
        let svc_err: ServiceError = db_err.into();
        assert!(matches!(svc_err, ServiceError::NotFound(ref msg) if msg == "task-1"));
    }

    #[test]
    fn test_db_error_other_maps_to_service_internal() {
        let db_err = flowstate_db::DbError::Internal("connection lost".into());
        let svc_err: ServiceError = db_err.into();
        assert!(matches!(svc_err, ServiceError::Internal(_)));
    }

    #[tokio::test]
    async fn local_service_project_crud() {
        let svc = make_service().await;

        // Create
        let input = CreateProject {
            name: "My Project".into(),
            slug: "my-project".into(),
            description: "A test project".into(),
            repo_url: String::new(),
        };
        let created = svc.create_project(&input).await.unwrap();
        assert_eq!(created.name, "My Project");
        assert_eq!(created.slug, "my-project");

        // Get
        let fetched = svc.get_project(&created.id).await.unwrap();
        assert_eq!(fetched.id, created.id);
        assert_eq!(fetched.name, "My Project");

        // Get by slug
        let by_slug = svc.get_project_by_slug("my-project").await.unwrap();
        assert_eq!(by_slug.id, created.id);

        // List (should have 1)
        let projects = svc.list_projects().await.unwrap();
        assert_eq!(projects.len(), 1);

        // Update
        let update = flowstate_core::project::UpdateProject {
            name: Some("Renamed".into()),
            ..Default::default()
        };
        let updated = svc.update_project(&created.id, &update).await.unwrap();
        assert_eq!(updated.name, "Renamed");

        // Delete
        svc.delete_project(&created.id).await.unwrap();

        // List (should have 0)
        let projects = svc.list_projects().await.unwrap();
        assert_eq!(projects.len(), 0);
    }

    #[tokio::test]
    async fn local_service_task_crud() {
        let svc = make_service().await;

        // Create a project first
        let project = svc
            .create_project(&CreateProject {
                name: "P1".into(),
                slug: "p1".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        // Create task
        let task = svc
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "Task 1".into(),
                description: "Do something".into(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .await
            .unwrap();
        assert_eq!(task.title, "Task 1");
        assert_eq!(task.status, Status::Todo);

        // Get
        let fetched = svc.get_task(&task.id).await.unwrap();
        assert_eq!(fetched.id, task.id);

        // List
        let filter = TaskFilter {
            project_id: Some(project.id.clone()),
            ..Default::default()
        };
        let tasks = svc.list_tasks(&filter).await.unwrap();
        assert_eq!(tasks.len(), 1);

        // Update
        let updated = svc
            .update_task(
                &task.id,
                &UpdateTask {
                    status: Some(Status::Build),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.status, Status::Build);

        // Delete
        svc.delete_task(&task.id).await.unwrap();
        let tasks = svc.list_tasks(&filter).await.unwrap();
        assert_eq!(tasks.len(), 0);
    }

    #[tokio::test]
    async fn local_service_sprint_crud() {
        let svc = make_service().await;

        // Create a project first
        let project = svc
            .create_project(&CreateProject {
                name: "P2".into(),
                slug: "p2".into(),
                description: String::new(),
                repo_url: String::new(),
            })
            .await
            .unwrap();

        // Create sprint
        let sprint = svc
            .create_sprint(&CreateSprint {
                project_id: project.id.clone(),
                name: "Sprint 1".into(),
                goal: "Ship it".into(),
                starts_at: None,
                ends_at: None,
            })
            .await
            .unwrap();
        assert_eq!(sprint.name, "Sprint 1");

        // Get
        let fetched = svc.get_sprint(&sprint.id).await.unwrap();
        assert_eq!(fetched.id, sprint.id);

        // List
        let sprints = svc.list_sprints(&project.id).await.unwrap();
        assert_eq!(sprints.len(), 1);

        // Update
        let updated = svc
            .update_sprint(
                &sprint.id,
                &flowstate_core::sprint::UpdateSprint {
                    name: Some("Sprint 1 - Renamed".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Sprint 1 - Renamed");

        // Delete
        svc.delete_sprint(&sprint.id).await.unwrap();
        let sprints = svc.list_sprints(&project.id).await.unwrap();
        assert_eq!(sprints.len(), 0);
    }

    #[tokio::test]
    async fn local_service_not_found_error() {
        let svc = make_service().await;
        let err = svc.get_project("nonexistent").await.unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }
}
