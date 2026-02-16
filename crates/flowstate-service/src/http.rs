use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
use flowstate_core::task::{CreateTask, Task, TaskFilter, UpdateTask};
use flowstate_core::task_link::{CreateTaskLink, TaskLink};
use reqwest::blocking::{Client, RequestBuilder};
use reqwest::StatusCode;

use crate::{ServiceError, TaskService};

/// HTTP client implementation of TaskService.
/// Connects to a running flowstate-server.
pub struct HttpService {
    base_url: String,
    client: Client,
    api_key: Option<String>,
}

impl HttpService {
    pub fn new(base_url: &str) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            client: Client::new(),
            api_key: None,
        }
    }

    pub fn with_api_key(base_url: &str, key: String) -> Self {
        let base_url = base_url.trim_end_matches('/').to_string();
        Self {
            base_url,
            client: Client::new(),
            api_key: Some(key),
        }
    }

    fn with_auth(&self, builder: RequestBuilder) -> RequestBuilder {
        match &self.api_key {
            Some(key) => builder.header("Authorization", format!("Bearer {key}")),
            None => builder,
        }
    }

    /// Check if the server is reachable.
    /// Health endpoint is NOT authenticated.
    pub fn health_check(&self) -> Result<(), ServiceError> {
        let resp = self
            .client
            .get(format!("{}/api/health", self.base_url))
            .send()
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

    fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ServiceError> {
        let builder = self.client.get(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp)
    }

    fn get_text(&self, path: &str) -> Result<String, ServiceError> {
        let builder = self.client.get(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        let status = resp.status();
        if status.is_success() {
            resp.text()
                .map_err(|e| ServiceError::Internal(format!("read body: {e}")))
        } else {
            Err(parse_error_with_status(status, resp))
        }
    }

    fn post_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ServiceError> {
        let builder = self.client.post(format!("{}{path}", self.base_url)).json(body);
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp)
    }

    fn put_json<B: serde::Serialize, T: serde::de::DeserializeOwned>(
        &self,
        path: &str,
        body: &B,
    ) -> Result<T, ServiceError> {
        let builder = self.client.put(format!("{}{path}", self.base_url)).json(body);
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        handle_response(resp)
    }

    fn put_text(&self, path: &str, body: &str) -> Result<(), ServiceError> {
        let builder = self
            .client
            .put(format!("{}{path}", self.base_url))
            .header("Content-Type", "text/markdown")
            .body(body.to_string());
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp))
        }
    }

    fn delete_req(&self, path: &str) -> Result<(), ServiceError> {
        let builder = self.client.delete(format!("{}{path}", self.base_url));
        let resp = self
            .with_auth(builder)
            .send()
            .map_err(|e| ServiceError::Internal(e.to_string()))?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(parse_error(resp))
        }
    }

    // -- Claude convenience methods (not on trait) --

    pub fn trigger_claude_run(
        &self,
        task_id: &str,
        action: &str,
    ) -> Result<ClaudeRun, ServiceError> {
        self.post_json(
            &format!("/api/tasks/{task_id}/claude-runs"),
            &serde_json::json!({ "action": action }),
        )
    }

    pub fn get_claude_run_output(&self, run_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/claude-runs/{run_id}/output"))
    }

    pub fn read_task_spec(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/spec"))
    }

    pub fn write_task_spec(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/spec"), content)
    }

    pub fn read_task_plan(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/plan"))
    }

    pub fn write_task_plan(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/plan"), content)
    }
}

fn handle_response<T: serde::de::DeserializeOwned>(
    resp: reqwest::blocking::Response,
) -> Result<T, ServiceError> {
    let status = resp.status();
    if status.is_success() {
        resp.json::<T>()
            .map_err(|e| ServiceError::Internal(format!("json decode: {e}")))
    } else {
        Err(parse_error_with_status(status, resp))
    }
}

fn parse_error(resp: reqwest::blocking::Response) -> ServiceError {
    let status = resp.status();
    parse_error_with_status(status, resp)
}

fn parse_error_with_status(
    status: StatusCode,
    resp: reqwest::blocking::Response,
) -> ServiceError {
    let body = resp.text().unwrap_or_default();
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

impl TaskService for HttpService {
    fn list_projects(&self) -> Result<Vec<Project>, ServiceError> {
        self.get_json("/api/projects")
    }

    fn get_project(&self, id: &str) -> Result<Project, ServiceError> {
        self.get_json(&format!("/api/projects/{id}"))
    }

    fn get_project_by_slug(&self, slug: &str) -> Result<Project, ServiceError> {
        self.get_json(&format!("/api/projects/by-slug/{slug}"))
    }

    fn create_project(&self, input: &CreateProject) -> Result<Project, ServiceError> {
        self.post_json("/api/projects", input)
    }

    fn update_project(&self, id: &str, update: &UpdateProject) -> Result<Project, ServiceError> {
        self.put_json(&format!("/api/projects/{id}"), update)
    }

    fn delete_project(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/projects/{id}"))
    }

    fn list_tasks(&self, filter: &TaskFilter) -> Result<Vec<Task>, ServiceError> {
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
        self.get_json(&format!("/api/tasks{qs}"))
    }

    fn get_task(&self, id: &str) -> Result<Task, ServiceError> {
        self.get_json(&format!("/api/tasks/{id}"))
    }

    fn create_task(&self, input: &CreateTask) -> Result<Task, ServiceError> {
        self.post_json("/api/tasks", input)
    }

    fn update_task(&self, id: &str, update: &UpdateTask) -> Result<Task, ServiceError> {
        self.put_json(&format!("/api/tasks/{id}"), update)
    }

    fn delete_task(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/tasks/{id}"))
    }

    fn count_tasks_by_status(
        &self,
        project_id: &str,
    ) -> Result<Vec<(String, i64)>, ServiceError> {
        self.get_json(&format!(
            "/api/tasks/count-by-status?project_id={project_id}"
        ))
    }

    fn list_child_tasks(&self, parent_id: &str) -> Result<Vec<Task>, ServiceError> {
        self.get_json(&format!("/api/tasks/{parent_id}/children"))
    }

    fn create_task_link(&self, input: &CreateTaskLink) -> Result<TaskLink, ServiceError> {
        self.post_json("/api/task-links", input)
    }

    fn list_task_links(&self, task_id: &str) -> Result<Vec<TaskLink>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/links"))
    }

    fn delete_task_link(&self, id: &str) -> Result<(), ServiceError> {
        self.delete_req(&format!("/api/task-links/{id}"))
    }

    fn create_claude_run(&self, input: &CreateClaudeRun) -> Result<ClaudeRun, ServiceError> {
        self.post_json(&format!("/api/tasks/{}/claude-runs", input.task_id), input)
    }

    fn get_claude_run(&self, id: &str) -> Result<ClaudeRun, ServiceError> {
        self.get_json(&format!("/api/claude-runs/{id}"))
    }

    fn list_claude_runs(&self, task_id: &str) -> Result<Vec<ClaudeRun>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/claude-runs"))
    }

    fn list_attachments(&self, task_id: &str) -> Result<Vec<Attachment>, ServiceError> {
        self.get_json(&format!("/api/tasks/{task_id}/attachments"))
    }
}
