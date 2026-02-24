use flowstate_core::task::{CreateTask, Priority, Status, TaskFilter, UpdateTask};
use flowstate_service::HttpService;
use serde_json::json;

use crate::protocol::{ToolDefinition, ToolResult};

/// Return the list of MCP tool definitions.
pub fn tool_definitions() -> Vec<ToolDefinition> {
    vec![
        ToolDefinition {
            name: "list_tasks".into(),
            description: "List tasks, optionally filtered by project_id and/or status.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string", "description": "Filter by project ID" },
                    "status": { "type": "string", "description": "Filter by status (todo, research, design, plan, build, verify, done)" }
                }
            }),
        },
        ToolDefinition {
            name: "get_task".into(),
            description: "Get a single task by ID.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string", "description": "The task ID" }
                },
                "required": ["task_id"]
            }),
        },
        ToolDefinition {
            name: "create_task".into(),
            description: "Create a new task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": { "type": "string" },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "parent_id": { "type": "string", "description": "Optional parent task ID for subtasks" },
                    "status": { "type": "string", "description": "Initial status (default: todo)" },
                    "priority": { "type": "string", "description": "Priority (urgent, high, medium, low, none; default: medium)" }
                },
                "required": ["project_id", "title"]
            }),
        },
        ToolDefinition {
            name: "update_task".into(),
            description: "Update fields on an existing task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" },
                    "title": { "type": "string" },
                    "description": { "type": "string" },
                    "status": { "type": "string" },
                    "priority": { "type": "string" }
                },
                "required": ["task_id"]
            }),
        },
        ToolDefinition {
            name: "list_child_tasks".into(),
            description: "List child tasks (subtasks) of a parent task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "parent_id": { "type": "string", "description": "The parent task ID" }
                },
                "required": ["parent_id"]
            }),
        },
        ToolDefinition {
            name: "get_task_spec".into(),
            description: "Read the specification (design doc) for a task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }),
        },
        ToolDefinition {
            name: "get_task_plan".into(),
            description: "Read the implementation plan for a task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }),
        },
        ToolDefinition {
            name: "get_task_research".into(),
            description: "Read the research notes for a task.".into(),
            input_schema: json!({
                "type": "object",
                "properties": {
                    "task_id": { "type": "string" }
                },
                "required": ["task_id"]
            }),
        },
    ]
}

/// Dispatch a tool call by name, returning a ToolResult.
pub async fn dispatch_tool(
    service: &HttpService,
    name: &str,
    args: &serde_json::Value,
) -> ToolResult {
    match name {
        "list_tasks" => handle_list_tasks(service, args).await,
        "get_task" => handle_get_task(service, args).await,
        "create_task" => handle_create_task(service, args).await,
        "update_task" => handle_update_task(service, args).await,
        "list_child_tasks" => handle_list_child_tasks(service, args).await,
        "get_task_spec" => handle_get_task_spec(service, args).await,
        "get_task_plan" => handle_get_task_plan(service, args).await,
        "get_task_research" => handle_get_task_research(service, args).await,
        _ => ToolResult::error(format!("unknown tool: {name}")),
    }
}

fn require_str<'a>(args: &'a serde_json::Value, key: &str) -> Result<&'a str, ToolResult> {
    args.get(key)
        .and_then(|v| v.as_str())
        .ok_or_else(|| ToolResult::error(format!("missing required parameter: {key}")))
}

async fn handle_list_tasks(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let filter = TaskFilter {
        project_id: args.get("project_id").and_then(|v| v.as_str()).map(String::from),
        status: args
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(Status::parse_str),
        ..Default::default()
    };
    match flowstate_service::TaskService::list_tasks(service, &filter).await {
        Ok(tasks) => match serde_json::to_string_pretty(&tasks) {
            Ok(json) => ToolResult::text(json),
            Err(e) => ToolResult::error(format!("serialization error: {e}")),
        },
        Err(e) => ToolResult::error(format!("list_tasks failed: {e}")),
    }
}

async fn handle_get_task(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let task_id = match require_str(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match flowstate_service::TaskService::get_task(service, task_id).await {
        Ok(task) => match serde_json::to_string_pretty(&task) {
            Ok(json) => ToolResult::text(json),
            Err(e) => ToolResult::error(format!("serialization error: {e}")),
        },
        Err(e) => ToolResult::error(format!("get_task failed: {e}")),
    }
}

async fn handle_create_task(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let project_id = match require_str(args, "project_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let title = match require_str(args, "title") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let description = args
        .get("description")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    let status = args
        .get("status")
        .and_then(|v| v.as_str())
        .and_then(Status::parse_str)
        .unwrap_or(Status::Todo);
    let priority = args
        .get("priority")
        .and_then(|v| v.as_str())
        .and_then(Priority::parse_str)
        .unwrap_or(Priority::Medium);
    let parent_id = args
        .get("parent_id")
        .and_then(|v| v.as_str())
        .map(String::from);

    let input = CreateTask {
        project_id: project_id.to_string(),
        title: title.to_string(),
        description,
        status,
        priority,
        parent_id,
        reviewer: String::new(),
        research_capability: None,
        design_capability: None,
        plan_capability: None,
        build_capability: None,
        verify_capability: None,
    };
    match flowstate_service::TaskService::create_task(service, &input).await {
        Ok(task) => match serde_json::to_string_pretty(&task) {
            Ok(json) => ToolResult::text(json),
            Err(e) => ToolResult::error(format!("serialization error: {e}")),
        },
        Err(e) => ToolResult::error(format!("create_task failed: {e}")),
    }
}

async fn handle_update_task(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let task_id = match require_str(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    let update = UpdateTask {
        title: args.get("title").and_then(|v| v.as_str()).map(String::from),
        description: args
            .get("description")
            .and_then(|v| v.as_str())
            .map(String::from),
        status: args
            .get("status")
            .and_then(|v| v.as_str())
            .and_then(Status::parse_str),
        priority: args
            .get("priority")
            .and_then(|v| v.as_str())
            .and_then(Priority::parse_str),
        ..Default::default()
    };
    match flowstate_service::TaskService::update_task(service, task_id, &update).await {
        Ok(task) => match serde_json::to_string_pretty(&task) {
            Ok(json) => ToolResult::text(json),
            Err(e) => ToolResult::error(format!("serialization error: {e}")),
        },
        Err(e) => ToolResult::error(format!("update_task failed: {e}")),
    }
}

async fn handle_list_child_tasks(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let parent_id = match require_str(args, "parent_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match flowstate_service::TaskService::list_child_tasks(service, parent_id).await {
        Ok(tasks) => match serde_json::to_string_pretty(&tasks) {
            Ok(json) => ToolResult::text(json),
            Err(e) => ToolResult::error(format!("serialization error: {e}")),
        },
        Err(e) => ToolResult::error(format!("list_child_tasks failed: {e}")),
    }
}

async fn handle_get_task_spec(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let task_id = match require_str(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match service.read_task_spec(task_id).await {
        Ok(content) => ToolResult::text(content),
        Err(e) => ToolResult::error(format!("get_task_spec failed: {e}")),
    }
}

async fn handle_get_task_plan(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let task_id = match require_str(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match service.read_task_plan(task_id).await {
        Ok(content) => ToolResult::text(content),
        Err(e) => ToolResult::error(format!("get_task_plan failed: {e}")),
    }
}

async fn handle_get_task_research(service: &HttpService, args: &serde_json::Value) -> ToolResult {
    let task_id = match require_str(args, "task_id") {
        Ok(v) => v,
        Err(e) => return e,
    };
    match service.read_task_research(task_id).await {
        Ok(content) => ToolResult::text(content),
        Err(e) => ToolResult::error(format!("get_task_research failed: {e}")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_definitions_has_expected_count() {
        let tools = tool_definitions();
        assert_eq!(tools.len(), 8);
    }

    #[test]
    fn tool_definitions_have_valid_schemas() {
        for tool in tool_definitions() {
            assert!(!tool.name.is_empty());
            assert!(!tool.description.is_empty());
            assert_eq!(tool.input_schema["type"], "object");
        }
    }

    #[test]
    fn tool_names_are_unique() {
        let tools = tool_definitions();
        let mut names: Vec<&str> = tools.iter().map(|t| t.name.as_str()).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), tools.len());
    }

    #[test]
    fn require_str_present() {
        let args = json!({"task_id": "abc"});
        assert_eq!(require_str(&args, "task_id").unwrap(), "abc");
    }

    #[test]
    fn require_str_missing() {
        let args = json!({});
        let err = require_str(&args, "task_id").unwrap_err();
        assert!(err.is_error);
        assert!(err.content[0].text.contains("task_id"));
    }

    #[test]
    fn require_str_wrong_type() {
        let args = json!({"task_id": 123});
        let err = require_str(&args, "task_id").unwrap_err();
        assert!(err.is_error);
    }
}
