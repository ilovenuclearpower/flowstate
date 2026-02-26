use std::io::BufRead;

use flowstate_service::HttpService;
use serde_json::json;
use tracing::{debug, error};

use crate::protocol::{JsonRpcRequest, JsonRpcResponse};
use crate::tools;

/// Run the MCP server loop, reading JSON-RPC from stdin and writing to stdout.
pub async fn run(service: HttpService) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let reader = stdin.lock();

    for line in reader.lines() {
        let line = match line {
            Ok(l) => l,
            Err(e) => {
                error!("stdin read error: {e}");
                break;
            }
        };

        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let request: JsonRpcRequest = match serde_json::from_str(trimmed) {
            Ok(r) => r,
            Err(e) => {
                let resp = JsonRpcResponse::error(None, -32700, format!("Parse error: {e}"));
                write_response(&resp);
                continue;
            }
        };

        debug!("MCP request: method={}", request.method);

        let response = handle_request(&service, &request).await;
        write_response(&response);
    }

    Ok(())
}

async fn handle_request(service: &HttpService, req: &JsonRpcRequest) -> JsonRpcResponse {
    match req.method.as_str() {
        "initialize" => JsonRpcResponse::success(
            req.id.clone(),
            json!({
                "protocolVersion": "2024-11-05",
                "capabilities": {
                    "tools": {}
                },
                "serverInfo": {
                    "name": "flowstate-mcp",
                    "version": env!("CARGO_PKG_VERSION")
                }
            }),
        ),
        "notifications/initialized" => {
            // Client acknowledgment — no response needed for notifications,
            // but we return success if an id was provided.
            JsonRpcResponse::success(req.id.clone(), json!({}))
        }
        "tools/list" => {
            let defs = tools::tool_definitions();
            JsonRpcResponse::success(req.id.clone(), json!({ "tools": defs }))
        }
        "tools/call" => {
            let tool_name = req
                .params
                .get("name")
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let arguments = req.params.get("arguments").cloned().unwrap_or(json!({}));
            let result = tools::dispatch_tool(service, tool_name, &arguments).await;
            JsonRpcResponse::success(req.id.clone(), serde_json::to_value(result).unwrap())
        }
        _ => JsonRpcResponse::error(
            req.id.clone(),
            -32601,
            format!("Method not found: {}", req.method),
        ),
    }
}

fn write_response(resp: &JsonRpcResponse) {
    if let Ok(json) = serde_json::to_string(resp) {
        println!("{json}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_request(method: &str, params: serde_json::Value) -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            id: Some(json!(1)),
            method: method.into(),
            params,
        }
    }

    #[tokio::test]
    async fn initialize_returns_server_info() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request("initialize", json!({}));
        let resp = handle_request(&svc, &req).await;
        let result = resp.result.unwrap();
        assert_eq!(result["serverInfo"]["name"], "flowstate-mcp");
        assert!(result["capabilities"]["tools"].is_object());
    }

    #[tokio::test]
    async fn tools_list_returns_tools() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request("tools/list", json!({}));
        let resp = handle_request(&svc, &req).await;
        let result = resp.result.unwrap();
        let tools = result["tools"].as_array().unwrap();
        assert_eq!(tools.len(), 8);
    }

    #[tokio::test]
    async fn unknown_method_returns_error() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request("bogus/method", json!({}));
        let resp = handle_request(&svc, &req).await;
        assert!(resp.error.is_some());
        assert_eq!(resp.error.unwrap().code, -32601);
    }

    #[tokio::test]
    async fn tools_call_unknown_tool_returns_error_in_result() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request(
            "tools/call",
            json!({"name": "nonexistent", "arguments": {}}),
        );
        let resp = handle_request(&svc, &req).await;
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
    }

    #[tokio::test]
    async fn tools_call_missing_param_returns_error_in_result() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request("tools/call", json!({"name": "get_task", "arguments": {}}));
        let resp = handle_request(&svc, &req).await;
        let result = resp.result.unwrap();
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("task_id"));
    }

    #[tokio::test]
    async fn notifications_initialized_returns_success() {
        let svc = HttpService::new("http://localhost:0");
        let req = make_request("notifications/initialized", json!({}));
        let resp = handle_request(&svc, &req).await;
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }
}
