use async_trait::async_trait;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::sprint::{CreateSprint, Sprint, UpdateSprint};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use flowstate_core::task_pr::{CreateTaskPr, TaskPr};
use reqwest::{Client, RequestBuilder, StatusCode};

use crate::{ServiceError, TaskService};

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct SystemStatus {
    pub server: String,
    pub runners: Vec<RunnerStatus>,
}

#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RunnerStatus {
    pub runner_id: String,
    pub last_seen: String,
    pub connected: bool,
}

/// Runner utilization metrics sent during registration heartbeat.
#[derive(Debug, Clone)]
pub struct RunnerUtilization {
    pub poll_interval: u64,
    pub max_concurrent: usize,
    pub max_builds: usize,
    pub active_count: usize,
    pub active_builds: usize,
    pub status: Option<String>,
}

/// Pending configuration changes from the server.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct PendingConfigResponse {
    #[serde(default)]
    pub poll_interval: Option<u64>,
    #[serde(default)]
    pub drain: Option<bool>,
}

/// Response from the register endpoint.
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize)]
pub struct RegisterResponse {
    pub status: String,
    pub runner_id: String,
    #[serde(default)]
    pub pending_config: Option<PendingConfigResponse>,
}

/// Async HTTP client implementation of TaskService.
/// Connects to a running flowstate-server.
pub struct HttpService {
    base_url: String,
    client: Client,
    api_key: Option<String>,
    runner_id: Option<String>,
}

impl HttpService {
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            client: Client::new(),
            api_key: None,
            runner_id: None,
        }
    }

    pub fn with_api_key(base_url: &str, key: String) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            client: Client::new(),
            api_key: Some(key),
            runner_id: None,
        }
    }

    pub fn set_runner_id(&mut self, id: String) {
        self.runner_id = Some(id);
    }

    fn with_auth(&self, builder: RequestBuilder) -> RequestBuilder {
        let builder = match &self.api_key {
            Some(key) => builder.header("Authorization", format!("Bearer {key}")),
            None => builder,
        };
        match &self.runner_id {
            Some(id) => builder.header("X-Runner-Id", id.as_str()),
            None => builder,
        }
    }

    /// Check if the server is reachable.
    /// Health endpoint is NOT authenticated.
    pub async fn health_check(&self) -> Result<(), ServiceError> {
        let resp = self
            .client
            .get(format!("{}/api/health", self.base_url))
            .send()
            .await
            .map_err(|e| ServiceError::Internal(format!("connection failed: {e}")))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(ServiceError::Internal(format!(
                "health check failed: {}",
                resp.status()
            )))
        }
    }

    async fn get_json<T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
    ) -> Result<T, ServiceError> {
        let builder = self.client.get(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp).await
    }

    async fn get_text(&self, path: &str) -> Result<String, ServiceError> {
        let builder = self.client.get(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            resp.text()
                .await
                .map_err(|e| ServiceError::Internal(format!("read body: {e}")))
        } else {
            Err(parse_error_with_status(status, resp).await)
        }
    }

    async fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ServiceError> {
        let builder = self
            .client
            .post(format!("{}{path}", self.base_url))
            .json(body);
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp).await
    }

    async fn put_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ServiceError> {
        let builder = self
            .client
            .put(format!("{}{path}", self.base_url))
            .json(body);
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp).await
    }

    async fn put_text(&self, path: &str, body: &str) -> Result<(), ServiceError> {
        let builder = self
            .client
            .put(format!("{}{path}", self.base_url))
            .header("Content-Type", "text/markdown")
            .body(body.to_string());
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp).await)
        }
    }

    async fn delete_req(&self, path: &str) -> Result<(), ServiceError> {
        let builder = self.client.delete(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp).await)
        }
    }

    // -- Claude convenience methods (not on trait) --

    pub async fn trigger_claude_run(
        &self,
        task_id: &str,
        action: &str,
    ) -> Result<ClaudeRun, ServiceError> {
        self.post_json(
            &format!("/api/tasks/{task_id}/claude-runs"),
            &serde_json::json!({ "action": action }),
        )
        .await
    }

    /// Trigger a run on any task with an optional capability requirement.
    /// Used by the runner to trigger follow-up phases on tasks it doesn't
    /// currently own (e.g., triggering Build on newly created subtasks).
    pub async fn trigger_task_run(
        &self,
        task_id: &str,
        action: &str,
        capability: Option<&str>,
    ) -> Result<ClaudeRun, ServiceError> {
        let mut body = serde_json::json!({ "action": action });
        if let Some(cap) = capability {
            body["required_capability"] = serde_json::Value::String(cap.to_string());
        }
        self.post_json(&format!("/api/tasks/{task_id}/claude-runs"), &body)
            .await
    }

    pub async fn get_claude_run_output(&self, run_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/claude-runs/{run_id}/output"))
            .await
    }

    pub async fn read_task_spec(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/spec")).await
    }

    pub async fn write_task_spec(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/spec"), content)
            .await
    }

    pub async fn read_task_plan(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/plan")).await
    }

    pub async fn write_task_plan(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/plan"), content)
            .await
    }

    pub async fn read_task_research(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/research"))
            .await
    }

    pub async fn write_task_research(
        &self,
        task_id: &str,
        content: &str,
    ) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/research"), content)
            .await
    }

    pub async fn read_task_verification(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/verification"))
            .await
    }

    pub async fn write_task_verification(
        &self,
        task_id: &str,
        content: &str,
    ) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/verification"), content)
            .await
    }

    /// Register this runner with the server, advertising its capabilities.
    /// When called without utilization, performs a simple registration.
    pub async fn register_runner(
        &self,
        runner_id: &str,
        backend_name: &str,
        capability: &str,
    ) -> Result<(), ServiceError> {
        self.register_runner_with_utilization(runner_id, backend_name, capability, None)
            .await
            .map(|_| ())
    }

    /// Register/heartbeat with utilization metrics. Returns any pending config from the server.
    pub async fn register_runner_with_utilization(
        &self,
        runner_id: &str,
        backend_name: &str,
        capability: &str,
        utilization: Option<&RunnerUtilization>,
    ) -> Result<RegisterResponse, ServiceError> {
        let mut body = serde_json::json!({
            "runner_id": runner_id,
            "backend_name": backend_name,
            "capability": capability,
        });

        if let Some(util) = utilization {
            body["poll_interval"] = serde_json::json!(util.poll_interval);
            body["max_concurrent"] = serde_json::json!(util.max_concurrent);
            body["max_builds"] = serde_json::json!(util.max_builds);
            body["active_count"] = serde_json::json!(util.active_count);
            body["active_builds"] = serde_json::json!(util.active_builds);
            if let Some(ref status) = util.status {
                body["status"] = serde_json::json!(status);
            }
        }

        let builder = self
            .client
            .post(format!("{}/api/runners/register", self.base_url));
        let resp = self
            .with_auth(builder)
            .json(&body)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;

        if resp.status().is_success() {
            resp.json::<RegisterResponse>()
                .await
                .map_err(|e| ServiceError::Internal(format!("json decode: {e}")))
        } else {
            Err(parse_error(resp).await)
        }
    }

    /// Claim the next queued claude run, atomically setting it to Running.
    /// Returns None if no queued runs exist (server returns 204).
    pub async fn claim_claude_run(&self) -> Result<Option<ClaudeRun>, ServiceError> {
        let builder = self
            .client
            .post(format!("{}/api/claude-runs/claim", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;

        if resp.status() == StatusCode::NO_CONTENT {
            return Ok(None);
        }
        if resp.status().is_success() {
            let run = resp
                .json::<ClaudeRun>()
                .await
                .map_err(|e| ServiceError::Internal(format!("json decode: {e}")))?;
            Ok(Some(run))
        } else {
            Err(parse_error(resp).await)
        }
    }

    /// Update the status of a claude run, optionally with PR info.
    pub async fn update_claude_run_status(
        &self,
        id: &str,
        status: &str,
        error_message: Option<&str>,
        exit_code: Option<i32>,
    ) -> Result<ClaudeRun, ServiceError> {
        self.put_json(
            &format!("/api/claude-runs/{id}/status"),
            &serde_json::json!({
                "status": status,
                "error_message": error_message,
                "exit_code": exit_code,
            }),
        )
        .await
    }

    /// Update the progress message on a running claude run.
    pub async fn update_claude_run_progress(
        &self,
        id: &str,
        message: &str,
    ) -> Result<(), ServiceError> {
        let builder = self
            .client
            .put(format!("{}/api/claude-runs/{id}/progress", self.base_url))
            .json(&serde_json::json!({ "message": message }));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp).await)
        }
    }

    /// Set the repo token for a project (encrypted server-side).
    pub async fn set_repo_token(&self, project_id: &str, token: &str) -> Result<(), ServiceError> {
        let builder = self
            .client
            .put(format!(
                "{}/api/projects/{project_id}/repo-token",
                self.base_url
            ))
            .json(&serde_json::json!({ "token": token }));
        let resp = self
            .with_auth(builder)
            .send()
            .await
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp).await)
        }
    }

    /// Get the decrypted repo token for a project (for runner use).
    pub async fn get_repo_token(&self, project_id: &str) -> Result<String, ServiceError> {
        let val: serde_json::Value = self
            .get_json(&format!("/api/projects/{project_id}/repo-token"))
            .await?;
        val["token"]
            .as_str()
            .map(String::from)
            .ok_or_else(|| ServiceError::Internal("missing token in response".into()))
    }

    /// Fetch system status (server + runner connectivity).
    pub async fn system_status(&self) -> Result<SystemStatus, ServiceError> {
        self.get_json("/api/status").await
    }

    /// Update a claude run with PR info (url, number, branch).
    pub async fn update_claude_run_pr(
        &self,
        id: &str,
        pr_url: Option<&str>,
        pr_number: Option<i64>,
        branch_name: Option<&str>,
    ) -> Result<ClaudeRun, ServiceError> {
        self.put_json(
            &format!("/api/claude-runs/{id}/status"),
            &serde_json::json!({
                "status": "completed",
                "pr_url": pr_url,
                "pr_number": pr_number,
                "branch_name": branch_name,
            }),
        )
        .await
    }
}

async fn handle_response<T: serde::de::DeserializeOwned>(
    resp: reqwest::Response,
) -> Result<T, ServiceError> {
    let status = resp.status();
    if status.is_success() {
        resp.json::<T>()
            .await
            .map_err(|e| ServiceError::Internal(format!("json decode: {e}")))
    } else {
        Err(parse_error_with_status(status, resp).await)
    }
}

async fn parse_error(resp: reqwest::Response) -> ServiceError {
    let status = resp.status();
    parse_error_with_status(status, resp).await
}

async fn parse_error_with_status(status: StatusCode, resp: reqwest::Response) -> ServiceError {
    let body = resp.text().await.unwrap_or_default();
    let msg = serde_json::from_str::<serde_json::Value>(&body)
        .ok()
        .and_then(|v| v["error"].as_str().map(String::from))
        .unwrap_or(body);

    if status == StatusCode::NOT_FOUND {
        ServiceError::NotFound(msg)
    } else if status == StatusCode::BAD_REQUEST {
        ServiceError::InvalidInput(msg)
    } else {
        ServiceError::Internal(msg)
    }
}

#[async_trait]
impl TaskService for HttpService {
    async fn list_projects(&self) -> Result<Vec<Project>, ServiceError> {
        self.get_json("/api/projects").await
    }

    async fn get_project(&self, id: &str) -> Result<Project, ServiceError> {
        self.get_json(&format!("/api/projects/{id}")).await
    }

    async fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError> {
        self.get_json(&format!("/api/projects/by-slug/{slug}"))
            .await
    }

    async fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError> {
        self.post_json("/api/projects", input).await
    }

    async fn update_project(
        &self,
        id: &str,
        update: &UpdateProject,
    ) -> Result<Project, ServiceError> {
        self.put_json(&format!("/api/projects/{id}"), update).await
    }

    async fn delete_project(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/projects/{id}")).await
    }

    async fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError> {
        let mut params = Vec::new();
        if let Some(ref pid) = filter.project_id {
            params.push(format!("project_id={pid}"));
        }
        if let Some(status) = filter.status {
            params.push(format!("status={}", status.as_str()));
        }
        if let Some(priority) = filter.priority {
            params.push(format!("priority={}", priority.as_str()));
        }
        if let Some(ref sid) = filter.sprint_id {
            params.push(format!("sprint_id={sid}"));
        }
        if let Some(limit) = filter.limit {
            params.push(format!("limit={limit}"));
        }
        let qs = if params.is_empty() {
            String::new()
        } else {
            format!("?{}", params.join("&"))
        };
        self.get_json(&format!("/api/tasks{qs}")).await
    }

    async fn get_task(&self, id: &str) -> Result<Task, ServiceError> {
        self.get_json(&format!("/api/tasks/{id}")).await
    }

    async fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError> {
        self.post_json("/api/tasks", input).await
    }

    async fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError> {
        self.put_json(&format!("/api/tasks/{id}"), update).await
    }

    async fn delete_task(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/tasks/{id}")).await
    }

    async fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError> {
        self.get_json(&format!(
            "/api/tasks/count-by-status?project_id={project_id}"
        ))
        .await
    }

    async fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError> {
        self.get_json(&format!("/api/tasks/{parent_id}/children"))
            .await
    }

    async fn create_sprint(&self, input: &CreateSprint) -> Result<Sprint, ServiceError> {
        self.post_json("/api/sprints", input).await
    }

    async fn get_sprint(&self, id: &str) -> Result<Sprint, ServiceError> {
        self.get_json(&format!("/api/sprints/{id}")).await
    }

    async fn list_sprints(&self, project_id: &str) -> Result<Vec<Sprint>, ServiceError> {
        self.get_json(&format!("/api/sprints?project_id={project_id}"))
            .await
    }

    async fn update_sprint(&self, id: &str, update: &UpdateSprint) -> Result<Sprint, ServiceError> {
        self.put_json(&format!("/api/sprints/{id}"), update).await
    }

    async fn delete_sprint(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/sprints/{id}")).await
    }

    async fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError> {
        self.post_json("/api/task-links", input).await
    }

    async fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/links")).await
    }

    async fn delete_task_link(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/task-links/{id}")).await
    }

    async fn create_task_pr(&self, input: &CreateTaskPr) -> Result<TaskPr, ServiceError> {
        self.post_json(&format!("/api/tasks/{}/prs", input.task_id), input)
            .await
    }

    async fn list_task_prs(&self, task_id: &str) -> Result<Vec<TaskPr>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/prs")).await
    }

    async fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError> {
        self.post_json(&format!("/api/tasks/{}/claude-runs", input.task_id), input)
            .await
    }

    async fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError> {
        self.get_json(&format!("/api/claude-runs/{id}")).await
    }

    async fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/claude-runs"))
            .await
    }

    async fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/attachments"))
            .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flowstate_core::claude_run::{ClaudeAction, ClaudeRunStatus, CreateClaudeRun};
    use flowstate_core::project::CreateProject;
    use flowstate_core::sprint::CreateSprint;
    use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
    use flowstate_core::task_link::{CreateTaskLink, LinkType};
    use flowstate_core::task_pr::CreateTaskPr;

    /// Spawn a test server and return an HttpService connected to it.
    /// The returned TestServer must be kept alive for the duration of the test.
    async fn setup() -> (HttpService, flowstate_server::test_helpers::TestServer) {
        let server = flowstate_server::test_helpers::spawn_test_server().await;
        let svc = HttpService::new(&server.base_url);
        (svc, server)
    }

    fn test_project() -> CreateProject {
        CreateProject {
            name: "Test Project".into(),
            slug: "test-project".into(),
            description: "A test project".into(),
            repo_url: String::new(),
        }
    }

    fn test_task(project_id: &str) -> CreateTask {
        CreateTask {
            project_id: project_id.to_string(),
            title: "Test Task".into(),
            description: "A test task".into(),
            status: Status::Todo,
            priority: Priority::Medium,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        }
    }

    // ---- health_check ----

    #[tokio::test]
    async fn health_check_succeeds() {
        let (svc, _server) = setup().await;
        svc.health_check().await.unwrap();
    }

    #[tokio::test]
    async fn health_check_unreachable() {
        let svc = HttpService::new("http://127.0.0.1:1");
        let err = svc.health_check().await.unwrap_err();
        assert!(matches!(err, ServiceError::Internal(_)));
    }

    // ---- constructors and setters ----

    #[tokio::test]
    async fn with_api_key_constructor() {
        let (_, server) = setup().await;
        let svc = HttpService::with_api_key(&server.base_url, "fake-key".into());
        // Server has no auth, so requests should still work
        svc.health_check().await.unwrap();
        let projects = svc.list_projects().await.unwrap();
        assert!(projects.is_empty());
    }

    #[tokio::test]
    async fn set_runner_id_propagated() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("test-runner".into());
        // Health check should still work with runner header
        svc.health_check().await.unwrap();
    }

    // ---- project CRUD ----

    #[tokio::test]
    async fn project_create_get_list_update_delete() {
        let (svc, _server) = setup().await;

        // Create
        let project = svc.create_project(&test_project()).await.unwrap();
        assert_eq!(project.name, "Test Project");
        assert_eq!(project.slug, "test-project");

        // Get by id
        let fetched = svc.get_project(&project.id).await.unwrap();
        assert_eq!(fetched.id, project.id);

        // Get by slug
        let by_slug = svc.get_project_by_slug("test-project").await.unwrap();
        assert_eq!(by_slug.id, project.id);

        // List
        let all = svc.list_projects().await.unwrap();
        assert_eq!(all.len(), 1);

        // Update
        let updated = svc
            .update_project(
                &project.id,
                &flowstate_core::project::UpdateProject {
                    name: Some("Renamed".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Renamed");

        // Delete
        svc.delete_project(&project.id).await.unwrap();
        let all = svc.list_projects().await.unwrap();
        assert!(all.is_empty());
    }

    // ---- task CRUD ----

    #[tokio::test]
    async fn task_create_get_list_update_delete() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        // Create
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        assert_eq!(task.title, "Test Task");

        // Get
        let fetched = svc.get_task(&task.id).await.unwrap();
        assert_eq!(fetched.id, task.id);

        // List with project filter
        let all = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(all.len(), 1);

        // Update
        let updated = svc
            .update_task(
                &task.id,
                &UpdateTask {
                    title: Some("Updated Title".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.title, "Updated Title");

        // Delete
        svc.delete_task(&task.id).await.unwrap();
        let all = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert!(all.is_empty());
    }

    // ---- task filters ----

    #[tokio::test]
    async fn list_tasks_with_status_filter() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        svc.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Todo".into(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::High,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();

        svc.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Done".into(),
            description: String::new(),
            status: Status::Done,
            priority: Priority::Low,
            parent_id: None,
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();

        // Filter by status
        let todo_only = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                status: Some(Status::Todo),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(todo_only.len(), 1);
        assert_eq!(todo_only[0].title, "Todo");

        // Filter by priority
        let high_only = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                priority: Some(Priority::High),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(high_only.len(), 1);
        assert_eq!(high_only[0].title, "Todo");

        // Filter with limit
        let limited = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                limit: Some(1),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(limited.len(), 1);
    }

    #[tokio::test]
    async fn list_tasks_with_sprint_filter() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        let sprint = svc
            .create_sprint(&CreateSprint {
                project_id: project.id.clone(),
                name: "Sprint 1".into(),
                goal: String::new(),
                starts_at: None,
                ends_at: None,
            })
            .await
            .unwrap();

        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        svc.update_task(
            &task.id,
            &UpdateTask {
                sprint_id: Some(Some(sprint.id.clone())),
                ..Default::default()
            },
        )
        .await
        .unwrap();

        let with_sprint = svc
            .list_tasks(&TaskFilter {
                project_id: Some(project.id.clone()),
                sprint_id: Some(sprint.id.clone()),
                ..Default::default()
            })
            .await
            .unwrap();
        assert_eq!(with_sprint.len(), 1);
    }

    // ---- count tasks by status ----

    #[tokio::test]
    async fn count_tasks_by_status_returns_counts() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        svc.create_task(&test_task(&project.id)).await.unwrap();

        let counts = svc.count_tasks_by_status(&project.id).await.unwrap();
        assert!(!counts.is_empty());
        // Should have at least one entry for "todo"
        let todo_count = counts.iter().find(|(s, _)| s == "todo");
        assert!(todo_count.is_some());
        assert_eq!(todo_count.unwrap().1, 1);
    }

    // ---- child tasks ----

    #[tokio::test]
    async fn list_child_tasks_empty_then_populated() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let parent = svc.create_task(&test_task(&project.id)).await.unwrap();

        // No children yet
        let children = svc.list_child_tasks(&parent.id).await.unwrap();
        assert!(children.is_empty());

        // Create a child
        svc.create_task(&CreateTask {
            project_id: project.id.clone(),
            title: "Child Task".into(),
            description: String::new(),
            status: Status::Todo,
            priority: Priority::Low,
            parent_id: Some(parent.id.clone()),
            reviewer: String::new(),
            research_capability: None,
            design_capability: None,
            plan_capability: None,
            build_capability: None,
            verify_capability: None,
        })
        .await
        .unwrap();

        let children = svc.list_child_tasks(&parent.id).await.unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].title, "Child Task");
    }

    // ---- sprint CRUD ----

    #[tokio::test]
    async fn sprint_create_get_list_update_delete() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

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

        let fetched = svc.get_sprint(&sprint.id).await.unwrap();
        assert_eq!(fetched.id, sprint.id);

        let all = svc.list_sprints(&project.id).await.unwrap();
        assert_eq!(all.len(), 1);

        let updated = svc
            .update_sprint(
                &sprint.id,
                &flowstate_core::sprint::UpdateSprint {
                    name: Some("Sprint 1 Updated".into()),
                    ..Default::default()
                },
            )
            .await
            .unwrap();
        assert_eq!(updated.name, "Sprint 1 Updated");

        svc.delete_sprint(&sprint.id).await.unwrap();
        let all = svc.list_sprints(&project.id).await.unwrap();
        assert!(all.is_empty());
    }

    // ---- task links ----

    #[tokio::test]
    async fn task_link_create_list_delete() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        let task1 = svc.create_task(&test_task(&project.id)).await.unwrap();
        let task2 = svc
            .create_task(&CreateTask {
                project_id: project.id.clone(),
                title: "Task 2".into(),
                description: String::new(),
                status: Status::Todo,
                priority: Priority::Medium,
                parent_id: None,
                reviewer: String::new(),
                research_capability: None,
                design_capability: None,
                plan_capability: None,
                build_capability: None,
                verify_capability: None,
            })
            .await
            .unwrap();

        let link = svc
            .create_task_link(&CreateTaskLink {
                source_task_id: task1.id.clone(),
                target_task_id: task2.id.clone(),
                link_type: LinkType::Blocks,
            })
            .await
            .unwrap();
        assert_eq!(link.source_task_id, task1.id);

        let links = svc.list_task_links(&task1.id).await.unwrap();
        assert_eq!(links.len(), 1);

        svc.delete_task_link(&link.id).await.unwrap();
        let links = svc.list_task_links(&task1.id).await.unwrap();
        assert!(links.is_empty());
    }

    // ---- task PRs ----

    #[tokio::test]
    async fn task_pr_create_list() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let pr = svc
            .create_task_pr(&CreateTaskPr {
                task_id: task.id.clone(),
                claude_run_id: None,
                pr_url: "https://github.com/org/repo/pull/42".into(),
                pr_number: 42,
                branch_name: "flowstate/test".into(),
            })
            .await
            .unwrap();
        assert_eq!(pr.pr_number, 42);

        let prs = svc.list_task_prs(&task.id).await.unwrap();
        assert_eq!(prs.len(), 1);
        assert_eq!(prs[0].pr_number, 42);
    }

    // ---- claude runs (trait methods) ----

    #[tokio::test]
    async fn claude_run_create_get_list() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let run = svc
            .create_claude_run(&CreateClaudeRun {
                task_id: task.id.clone(),
                action: ClaudeAction::Research,
                required_capability: None,
            })
            .await
            .unwrap();
        assert_eq!(run.task_id, task.id);
        assert_eq!(run.action, ClaudeAction::Research);
        assert_eq!(run.status, ClaudeRunStatus::Queued);

        let fetched = svc.get_claude_run(&run.id).await.unwrap();
        assert_eq!(fetched.id, run.id);

        let runs = svc.list_claude_runs(&task.id).await.unwrap();
        assert_eq!(runs.len(), 1);
    }

    // ---- convenience: trigger_claude_run ----

    #[tokio::test]
    async fn trigger_claude_run_creates_run() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();
        assert_eq!(run.task_id, task.id);
        assert_eq!(run.action, ClaudeAction::Research);
    }

    // ---- convenience: claim_claude_run ----

    #[tokio::test]
    async fn claim_claude_run_returns_none_when_empty() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("claimer".into());
        svc.register_runner("claimer", "claude-cli", "standard")
            .await
            .unwrap();

        let claimed = svc.claim_claude_run().await.unwrap();
        assert!(claimed.is_none());
    }

    #[tokio::test]
    async fn claim_claude_run_claims_queued_run() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("claimer".into());
        svc.register_runner("claimer", "claude-cli", "standard")
            .await
            .unwrap();

        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();

        let claimed = svc.claim_claude_run().await.unwrap();
        assert!(claimed.is_some());
        assert_eq!(claimed.unwrap().id, run.id);
    }

    // ---- convenience: update_claude_run_status ----

    #[tokio::test]
    async fn update_claude_run_status_to_completed() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("runner-1".into());
        svc.register_runner("runner-1", "claude-cli", "standard")
            .await
            .unwrap();

        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();

        // Claim first (moves to Running)
        svc.claim_claude_run().await.unwrap();

        let updated = svc
            .update_claude_run_status(&run.id, "completed", None, Some(0))
            .await
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Completed);
    }

    #[tokio::test]
    async fn update_claude_run_status_to_failed() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("runner-2".into());
        svc.register_runner("runner-2", "claude-cli", "standard")
            .await
            .unwrap();

        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();

        svc.claim_claude_run().await.unwrap();

        let updated = svc
            .update_claude_run_status(&run.id, "failed", Some("something broke"), Some(1))
            .await
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Failed);
        assert_eq!(updated.error_message.as_deref(), Some("something broke"));
    }

    // ---- convenience: update_claude_run_progress ----

    #[tokio::test]
    async fn update_claude_run_progress_succeeds() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("runner-3".into());
        svc.register_runner("runner-3", "claude-cli", "standard")
            .await
            .unwrap();

        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        svc.trigger_claude_run(&task.id, "research").await.unwrap();

        let claimed = svc.claim_claude_run().await.unwrap().unwrap();
        svc.update_claude_run_progress(&claimed.id, "Working on it...")
            .await
            .unwrap();
    }

    // ---- convenience: spec/plan/research/verification roundtrip ----

    #[tokio::test]
    async fn spec_plan_research_verification_roundtrip() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        // Spec
        svc.write_task_spec(&task.id, "# Spec").await.unwrap();
        let spec = svc.read_task_spec(&task.id).await.unwrap();
        assert_eq!(spec, "# Spec");

        // Plan
        svc.write_task_plan(&task.id, "# Plan").await.unwrap();
        let plan = svc.read_task_plan(&task.id).await.unwrap();
        assert_eq!(plan, "# Plan");

        // Research
        svc.write_task_research(&task.id, "# Research")
            .await
            .unwrap();
        let research = svc.read_task_research(&task.id).await.unwrap();
        assert_eq!(research, "# Research");

        // Verification
        svc.write_task_verification(&task.id, "# Verification")
            .await
            .unwrap();
        let verification = svc.read_task_verification(&task.id).await.unwrap();
        assert_eq!(verification, "# Verification");
    }

    // ---- convenience: repo token ----

    #[tokio::test]
    async fn repo_token_set_get_roundtrip() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();

        svc.set_repo_token(&project.id, "ghp_test_token_123")
            .await
            .unwrap();
        let token = svc.get_repo_token(&project.id).await.unwrap();
        assert_eq!(token, "ghp_test_token_123");
    }

    // ---- convenience: system_status ----

    #[tokio::test]
    async fn system_status_returns_ok() {
        let (svc, _server) = setup().await;
        let status = svc.system_status().await.unwrap();
        assert_eq!(status.server, "ok");
    }

    // ---- convenience: update_claude_run_pr ----

    #[tokio::test]
    async fn update_claude_run_pr_sets_pr_info() {
        let (mut svc, _server) = setup().await;
        svc.set_runner_id("pr-runner".into());
        svc.register_runner("pr-runner", "claude-cli", "standard")
            .await
            .unwrap();

        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();

        // Claim first
        svc.claim_claude_run().await.unwrap();

        let updated = svc
            .update_claude_run_pr(
                &run.id,
                Some("https://github.com/org/repo/pull/99"),
                Some(99),
                Some("flowstate/my-branch"),
            )
            .await
            .unwrap();
        assert_eq!(updated.status, ClaudeRunStatus::Completed);
    }

    // ---- convenience: register_runner ----

    #[tokio::test]
    async fn register_runner_succeeds() {
        let (svc, _server) = setup().await;
        svc.register_runner("runner-reg-test", "claude-cli", "standard")
            .await
            .unwrap();
    }

    // ---- convenience: get_claude_run_output ----

    #[tokio::test]
    async fn get_claude_run_output_not_found_when_no_output() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();
        let run = svc.trigger_claude_run(&task.id, "research").await.unwrap();

        // Output not available yet
        let err = svc.get_claude_run_output(&run.id).await.unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    // ---- attachments ----

    #[tokio::test]
    async fn list_attachments_empty() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let attachments = svc.list_attachments(&task.id).await.unwrap();
        assert!(attachments.is_empty());
    }

    // ---- error paths ----

    #[tokio::test]
    async fn get_nonexistent_project_returns_not_found() {
        let (svc, _server) = setup().await;
        let err = svc
            .get_project("00000000-0000-0000-0000-000000000000")
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn get_nonexistent_task_returns_not_found() {
        let (svc, _server) = setup().await;
        let err = svc
            .get_task("00000000-0000-0000-0000-000000000000")
            .await
            .unwrap_err();
        assert!(matches!(err, ServiceError::NotFound(_)));
    }

    #[tokio::test]
    async fn trigger_design_without_approved_research_returns_invalid_input() {
        let (svc, _server) = setup().await;
        let project = svc.create_project(&test_project()).await.unwrap();
        let task = svc.create_task(&test_task(&project.id)).await.unwrap();

        let err = svc
            .trigger_claude_run(&task.id, "design")
            .await
            .unwrap_err();
        assert!(
            matches!(err, ServiceError::InvalidInput(_)),
            "expected InvalidInput, got: {err:?}"
        );
    }

    // ---- base_url trailing slash trimming ----

    #[tokio::test]
    async fn trailing_slash_in_base_url_is_trimmed() {
        let server = flowstate_server::test_helpers::spawn_test_server().await;
        let url_with_slash = format!("{}/", server.base_url);
        let svc = HttpService::new(&url_with_slash);
        svc.health_check().await.unwrap();
    }
}
