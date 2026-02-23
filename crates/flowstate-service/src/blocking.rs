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

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::project::CreateProject;
    use flowstate_core::sprint::CreateSprint;
    use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
    use flowstate_core::task_link::{CreateTaskLink, LinkType};
    use flowstate_core::task_pr::CreateTaskPr;

    /// Spawn a test server on a background thread (since BlockingHttpService
    /// creates its own tokio runtime and cannot be nested inside another).
    /// Returns the base_url. The server stays alive indefinitely via
    /// `std::future::pending()`.
    fn spawn_blocking_server() -> String {
        let (tx, rx) = std::sync::mpsc::sync_channel(1);
        std::thread::spawn(move || {
            let rt = tokio::runtime::Runtime::new().unwrap();
            rt.block_on(async {
                let server =
                    flowstate_server::test_helpers::spawn_test_server().await;
                tx.send(server.base_url.clone()).unwrap();
                // Keep the server alive for the duration of the test
                std::future::pending::<()>().await;
            });
        });
        rx.recv().unwrap()
    }

    fn test_project() -> CreateProject {
        CreateProject {
            name: "Blocking Project".into(),
            slug: "blocking-project".into(),
            description: "A test project".into(),
            repo_url: String::new(),
        }
    }

    fn test_task(project_id: &str) -> CreateTask {
        CreateTask {
            project_id: project_id.to_string(),
            title: "Blocking Task".into(),
            description: "A test task".into(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
        }
    }

    // ---- health check ----

    #[test]
    fn blocking_health_check() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);
        svc.health_check().unwrap();
    }

    // ---- constructors ----

    #[test]
    fn blocking_with_api_key() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::with_api_key(&url, "fake-key".into());
        svc.health_check().unwrap();
    }

    // ---- project CRUD ----

    #[test]
    fn blocking_project_create_get_list_update_delete() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        assert_eq!(project.name, "Blocking Project");

        let fetched = svc.get_project(&project.id).unwrap();
        assert_eq!(fetched.id, project.id);

        let by_slug = svc.get_project_by_slug("blocking-project").unwrap();
        assert_eq!(by_slug.id, project.id);

        let all = svc.list_projects().unwrap();
        assert_eq!(all.len(), 1);

        let updated = svc
            .update_project(
                &project.id,
                &flowstate_core::project::UpdateProject {
                    name: Some("Renamed".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Renamed");

        svc.delete_project(&project.id).unwrap();
        assert!(svc.list_projects().unwrap().is_empty());
    }

    // ---- task CRUD ----

    #[test]
    fn blocking_task_create_get_list_update_delete() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();
        assert_eq!(task.title, "Blocking Task");

        let fetched = svc.get_task(&task.id).unwrap();
        assert_eq!(fetched.id, task.id);

        let all = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                ..Default::default()
            })
            .unwrap();
        assert_eq!(all.len(), 1);

        let updated = svc
            .update_task(
                &task.id,
                &UpdateTask {
                    title: Some("Updated".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.title, "Updated");

        let counts = svc.count_tasks_by_status(&project.id).unwrap();
        assert!(!counts.is_empty());

        let children = svc.list_child_tasks(&task.id).unwrap();
        assert!(children.is_empty());

        svc.delete_task(&task.id).unwrap();
    }

    // ---- sprint CRUD ----

    #[test]
    fn blocking_sprint_create_get_list_update_delete() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let sprint = svc
            .create_sprint(&CreateSprint {
                project_id: project.id.clone(),
                name: "Sprint 1".into(),
                goal: "Ship it".into(),
                starts_at: None,
                ends_at: None,
            })
            .unwrap();
        assert_eq!(sprint.name, "Sprint 1");

        let fetched = svc.get_sprint(&sprint.id).unwrap();
        assert_eq!(fetched.id, sprint.id);

        let all = svc.list_sprints(&project.id).unwrap();
        assert_eq!(all.len(), 1);

        let updated = svc
            .update_sprint(
                &sprint.id,
                &flowstate_core::sprint::UpdateSprint {
                    name: Some("Updated Sprint".into()),
                    ..Default::default()
                },
            )
            .unwrap();
        assert_eq!(updated.name, "Updated Sprint");

        svc.delete_sprint(&sprint.id).unwrap();
        assert!(svc.list_sprints(&project.id).unwrap().is_empty());
    }

    // ---- task links ----

    #[test]
    fn blocking_task_link_create_list_delete() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task1 = svc.create_task(&test_task(&project.id)).unwrap();
        let task2 = svc
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "Task 2".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
            })
            .unwrap();

        let link = svc
            .create_task_link(&CreateTaskLink {
                source_task_id: task1.id.clone(),
                target_task_id: task2.id.clone(),
                link_type: LinkType::Blocks,
            })
            .unwrap();
        assert_eq!(link.source_task_id, task1.id);

        let links = svc.list_task_links(&task1.id).unwrap();
        assert_eq!(links.len(), 1);

        svc.delete_task_link(&link.id).unwrap();
        assert!(svc.list_task_links(&task1.id).unwrap().is_empty());
    }

    // ---- task PRs ----

    #[test]
    fn blocking_task_pr_create_list() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();

        let pr = svc
            .create_task_pr(&CreateTaskPr {
                task_id: task.id.clone(),
                claude_run_id: None,
                pr_url: "https://github.com/org/repo/pull/7".into(),
                pr_number: 7,
                branch_name: "flowstate/blocking-test".into(),
            })
            .unwrap();
        assert_eq!(pr.pr_number, 7);

        let prs = svc.list_task_prs(&task.id).unwrap();
        assert_eq!(prs.len(), 1);
    }

    // ---- claude runs ----

    #[test]
    fn blocking_claude_run_create_get_list() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();

        let run = svc
            .create_claude_run(&flowstate_core::claude_run::CreateClaudeRun {
                task_id: task.id.clone(),
                action: flowstate_core::claude_run::ClaudeAction::Research,
                required_capability: None,
            })
            .unwrap();
        assert_eq!(run.task_id, task.id);

        let fetched = svc.get_claude_run(&run.id).unwrap();
        assert_eq!(fetched.id, run.id);

        let runs = svc.list_claude_runs(&task.id).unwrap();
        assert_eq!(runs.len(), 1);
    }

    // ---- content roundtrip ----

    #[test]
    fn blocking_spec_plan_research_verification_roundtrip() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();

        svc.write_task_spec(&task.id, "blocking spec").unwrap();
        assert_eq!(svc.read_task_spec(&task.id).unwrap(), "blocking spec");

        svc.write_task_plan(&task.id, "blocking plan").unwrap();
        assert_eq!(svc.read_task_plan(&task.id).unwrap(), "blocking plan");

        svc.write_task_research(&task.id, "blocking research")
            .unwrap();
        assert_eq!(
            svc.read_task_research(&task.id).unwrap(),
            "blocking research"
        );

        svc.write_task_verification(&task.id, "blocking verify")
            .unwrap();
        assert_eq!(
            svc.read_task_verification(&task.id).unwrap(),
            "blocking verify"
        );
    }

    // ---- repo token ----

    #[test]
    fn blocking_repo_token_roundtrip() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();

        svc.set_repo_token(&project.id, "ghp_blocking_token")
            .unwrap();
        assert_eq!(
            svc.get_repo_token(&project.id).unwrap(),
            "ghp_blocking_token"
        );
    }

    // ---- system status ----

    #[test]
    fn blocking_system_status() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);
        let status = svc.system_status().unwrap();
        assert_eq!(status.server, "ok");
    }

    // ---- trigger + output ----

    #[test]
    fn blocking_trigger_and_get_output_not_found() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();

        let run = svc.trigger_claude_run(&task.id, "research").unwrap();
        assert_eq!(run.task_id, task.id);

        // Output not available yet
        let err = svc.get_claude_run_output(&run.id).unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    // ---- attachments ----

    #[test]
    fn blocking_list_attachments_empty() {
        let url = spawn_blocking_server();
        let svc = BlockingHttpService::new(&url);

        let project = svc.create_project(&test_project()).unwrap();
        let task = svc.create_task(&test_task(&project.id)).unwrap();

        let attachments = svc.list_attachments(&task.id).unwrap();
        assert!(attachments.is_empty());
    }
}
