use async_trait::async_trait;
use flowstate_core::attachment::Attachment;
use flowstate_core::claude_run::{ClaudeRun, CreateClaudeRun};
use flowstate_core::project::{CreateProject, Project, UpdateProject};
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

    async fn get_json<T: serde::de::DeserializeOwned>(&self, path: &str) -> Result<T, ServiceError> {
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
        self.get_text(&format!("/api/tasks/{task_id}/research")).await
    }

    pub async fn write_task_research(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/research"), content).await
    }

    pub async fn read_task_verification(&self, task_id: &str) -> Result<String, ServiceError> {
        self.get_text(&format!("/api/tasks/{task_id}/verification")).await
    }

    pub async fn write_task_verification(&self, task_id: &str, content: &str) -> Result<(), ServiceError> {
        self.put_text(&format!("/api/tasks/{task_id}/verification"), content).await
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

async fn parse_error_with_status(
    status: StatusCode,
    resp: reqwest::Response,
) -> ServiceError {
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
