//! Local AXL A2A/MCP service surface for Anton's personal agent.

use std::sync::Arc;

use anton_core::axl::{DEFAULT_AXL_A2A_PORT, DEFAULT_AXL_ROUTER_PORT};
use anton_core::ens::normalize_chat_name;
use anton_core::transport::PeerId;
use axum::extract::State as AxumState;
use axum::response::IntoResponse;
use axum::routing::{delete, get, post};
use axum::{Json, Router};
use parking_lot::Mutex;
use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use tauri::{async_runtime::JoinHandle, AppHandle, Runtime, State};

use crate::agent;
use crate::chat::ResolverState;
use crate::sidecar::AxlSidecarState;

const SERVICE_NAME: &str = "anton_agent";

#[derive(Default)]
pub struct A2aServiceState {
    handles: Mutex<Vec<JoinHandle<()>>>,
}

impl A2aServiceState {
    pub fn start<R: Runtime>(&self, app: AppHandle<R>) {
        if !self.handles.lock().is_empty() {
            return;
        }

        let app = Arc::new(app);
        let a2a_app = app.clone();
        let router_app = app.clone();
        let a2a_handle = tauri::async_runtime::spawn(async move {
            if let Err(err) = serve_a2a(a2a_app).await {
                tracing::warn!(target: "anton::a2a", "A2A server stopped: {err}");
            }
        });
        let router_handle = tauri::async_runtime::spawn(async move {
            if let Err(err) = serve_mcp_router(router_app).await {
                tracing::warn!(target: "anton::a2a", "MCP router stopped: {err}");
            }
        });

        *self.handles.lock() = vec![a2a_handle, router_handle];
    }

    pub fn shutdown(&self) {
        for handle in self.handles.lock().drain(..) {
            handle.abort();
        }
    }
}

async fn serve_a2a<R: Runtime>(app: Arc<AppHandle<R>>) -> Result<(), String> {
    let router = Router::new()
        .route("/", post(a2a_post::<R>))
        .route("/a2a", post(a2a_post::<R>))
        .route("/.well-known/agent-card.json", get(agent_card))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", DEFAULT_AXL_A2A_PORT))
        .await
        .map_err(|e| format!("bind A2A :{DEFAULT_AXL_A2A_PORT}: {e}"))?;
    tracing::info!(target: "anton::a2a", "A2A server listening on 127.0.0.1:{DEFAULT_AXL_A2A_PORT}");
    axum::serve(listener, router)
        .await
        .map_err(|e| format!("serve A2A: {e}"))
}

async fn serve_mcp_router<R: Runtime>(app: Arc<AppHandle<R>>) -> Result<(), String> {
    let router = Router::new()
        .route("/route", post(mcp_route::<R>))
        .route("/health", get(|| async { Json(json!({ "ok": true })) }))
        .route("/services", get(services))
        .route("/register", post(register_service))
        .route("/register/{service}", delete(delete_service))
        .with_state(app);
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", DEFAULT_AXL_ROUTER_PORT))
        .await
        .map_err(|e| format!("bind MCP router :{DEFAULT_AXL_ROUTER_PORT}: {e}"))?;
    tracing::info!(target: "anton::a2a", "MCP router listening on 127.0.0.1:{DEFAULT_AXL_ROUTER_PORT}");
    axum::serve(listener, router)
        .await
        .map_err(|e| format!("serve MCP router: {e}"))
}

async fn agent_card() -> impl IntoResponse {
    Json(json!({
        "name": "Anton Personal Agent",
        "description": "ENS-backed personal chat agent reachable over AXL A2A.",
        "version": "0.1.0",
        "skills": [
            skill("draft_reply", "Draft a reply for an ENS conversation without sending it."),
            skill("send_reply", "Send a signed Anton chat reply on behalf of the local user."),
            skill("summarize_conversation", "Summarize recent local conversation context."),
            skill("handoff_to_human", "Disable agent mode and request manual human handling.")
        ]
    }))
}

fn skill(id: &str, description: &str) -> Value {
    json!({
        "id": id,
        "name": id,
        "description": description,
        "tags": ["anton", "chat", "agent"]
    })
}

async fn services() -> impl IntoResponse {
    Json(json!({
        "services": [{
            "service": SERVICE_NAME,
            "endpoint": format!("http://127.0.0.1:{DEFAULT_AXL_A2A_PORT}")
        }]
    }))
}

async fn register_service() -> impl IntoResponse {
    Json(json!({ "ok": true, "service": SERVICE_NAME }))
}

async fn delete_service() -> impl IntoResponse {
    Json(json!({ "ok": true }))
}

async fn a2a_post<R: Runtime>(
    AxumState(app): AxumState<Arc<AppHandle<R>>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    Json(handle_a2a_jsonrpc(&app, payload).await)
}

async fn mcp_route<R: Runtime>(
    AxumState(app): AxumState<Arc<AppHandle<R>>>,
    Json(payload): Json<Value>,
) -> impl IntoResponse {
    Json(handle_mcp_envelope(&app, payload).await)
}

async fn handle_a2a_jsonrpc<R: Runtime>(app: &AppHandle<R>, payload: Value) -> Value {
    let id = payload.get("id").cloned().unwrap_or(Value::Null);
    if payload.get("method").and_then(Value::as_str) != Some("SendMessage") {
        return jsonrpc_error(id, -32601, "Unsupported A2A method.");
    }
    let Some(text) = payload
        .pointer("/params/message/parts/0/text")
        .and_then(Value::as_str)
    else {
        return jsonrpc_error(id, -32602, "A2A SendMessage requires a text part.");
    };
    let Ok(envelope) = serde_json::from_str::<Value>(text) else {
        return jsonrpc_error(id, -32602, "A2A text part must be a JSON MCP envelope.");
    };
    let result = handle_mcp_envelope(app, envelope).await;
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": {
            "message": {
                "role": "ROLE_AGENT",
                "parts": [{ "text": result.to_string() }],
                "messageId": uuid::Uuid::new_v4().to_string()
            }
        }
    })
}

async fn handle_mcp_envelope<R: Runtime>(app: &AppHandle<R>, payload: Value) -> Value {
    let service = payload
        .get("service")
        .and_then(Value::as_str)
        .unwrap_or(SERVICE_NAME);
    if service != SERVICE_NAME {
        return jsonrpc_error(Value::Null, -32601, "Unknown Anton MCP service.");
    }
    let request = payload.get("request").cloned().unwrap_or(payload);
    handle_mcp_request(app, request).await
}

async fn handle_mcp_request<R: Runtime>(app: &AppHandle<R>, request: Value) -> Value {
    let id = request.get("id").cloned().unwrap_or(Value::Null);
    match request.get("method").and_then(Value::as_str) {
        Some("tools/list") => json!({
            "jsonrpc": "2.0",
            "id": id,
            "result": { "tools": tool_descriptors() }
        }),
        Some("tools/call") => match call_tool(app, &request).await {
            Ok(result) => json!({ "jsonrpc": "2.0", "id": id, "result": result }),
            Err(err) => jsonrpc_error(id, -32000, &err),
        },
        Some(other) => jsonrpc_error(id, -32601, &format!("Unsupported MCP method: {other}")),
        None => jsonrpc_error(id, -32600, "Missing JSON-RPC method."),
    }
}

fn tool_descriptors() -> Value {
    json!([
        {
            "name": "draft_reply",
            "description": "Draft a reply for an ENS peer without sending.",
            "inputSchema": {
                "type": "object",
                "properties": { "peer": { "type": "string" } },
                "required": ["peer"]
            }
        },
        {
            "name": "send_reply",
            "description": "Send a signed Anton chat reply.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "peer": { "type": "string" },
                    "text": { "type": "string" }
                },
                "required": ["peer"]
            }
        },
        {
            "name": "summarize_conversation",
            "description": "Summarize recent local conversation context.",
            "inputSchema": {
                "type": "object",
                "properties": { "peer": { "type": "string" } },
                "required": ["peer"]
            }
        },
        {
            "name": "handoff_to_human",
            "description": "Disable agent mode for an ENS peer.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "peer": { "type": "string" },
                    "reason": { "type": "string" }
                },
                "required": ["peer"]
            }
        }
    ])
}

async fn call_tool<R: Runtime>(app: &AppHandle<R>, request: &Value) -> Result<Value, String> {
    let name = request
        .pointer("/params/name")
        .and_then(Value::as_str)
        .ok_or_else(|| "tools/call requires params.name.".to_string())?;
    let arguments = request
        .pointer("/params/arguments")
        .cloned()
        .unwrap_or_else(|| json!({}));
    call_tool_by_name(app, name, arguments).await
}

async fn call_tool_by_name<R: Runtime>(
    app: &AppHandle<R>,
    name: &str,
    arguments: Value,
) -> Result<Value, String> {
    let peer = arguments
        .get("peer")
        .and_then(Value::as_str)
        .map(normalize_chat_name)
        .ok_or_else(|| "Tool arguments must include peer.".to_string())?;
    match name {
        "draft_reply" => serde_json::to_value(agent::draft_reply_for_peer(app, &peer).await?)
            .map_err(|e| e.to_string()),
        "send_reply" => {
            let text = arguments
                .get("text")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            serde_json::to_value(agent::send_reply_for_peer(app, &peer, text).await?)
                .map_err(|e| e.to_string())
        }
        "summarize_conversation" => {
            serde_json::to_value(agent::summarize_conversation_for_peer(app, &peer).await?)
                .map_err(|e| e.to_string())
        }
        "handoff_to_human" => {
            let reason = arguments
                .get("reason")
                .and_then(Value::as_str)
                .map(|s| s.to_string());
            serde_json::to_value(agent::handoff_to_human_for_peer(app, &peer, reason)?)
                .map_err(|e| e.to_string())
        }
        other => Err(format!("Unknown Anton agent tool: {other}")),
    }
}

fn jsonrpc_error(id: Value, code: i64, message: &str) -> Value {
    json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": {
            "code": code,
            "message": message
        }
    })
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aCallToolRequest {
    pub peer: String,
    pub tool: String,
    #[serde(default)]
    pub arguments: Value,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct A2aCallToolResponse {
    pub ok: bool,
    pub response: Value,
}

#[tauri::command]
pub async fn agent_a2a_call_tool(
    resolver: State<'_, ResolverState>,
    sidecar_state: State<'_, AxlSidecarState>,
    request: A2aCallToolRequest,
) -> Result<A2aCallToolResponse, String> {
    let resolved = resolver
        .0
        .resolve_forward(&request.peer)
        .await
        .map_err(|e| e.to_string())?;
    let peer_id = PeerId::from_hex(resolved.peer_id_hex.trim()).map_err(|e| e.to_string())?;
    let sidecar = sidecar_state
        .sidecar
        .lock()
        .clone()
        .ok_or_else(|| "AXL sidecar is not running.".to_string())?;

    let request_id = uuid::Uuid::new_v4().to_string();
    let mcp = json!({
        "service": SERVICE_NAME,
        "request": {
            "jsonrpc": "2.0",
            "method": "tools/call",
            "id": request_id,
            "params": {
                "name": request.tool,
                "arguments": request.arguments
            }
        }
    });
    let a2a = json!({
        "jsonrpc": "2.0",
        "method": "SendMessage",
        "id": uuid::Uuid::new_v4().to_string(),
        "params": {
            "message": {
                "role": "ROLE_USER",
                "parts": [{ "text": mcp.to_string() }],
                "messageId": uuid::Uuid::new_v4().to_string()
            }
        }
    });
    let response = sidecar
        .transport()
        .client()
        .a2a_call(&peer_id, &a2a)
        .await
        .map_err(|e| e.to_string())?;
    Ok(A2aCallToolResponse { ok: true, response })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn mcp_tools_list_has_expected_skills() {
        let tools = tool_descriptors();
        let names: Vec<_> = tools
            .as_array()
            .unwrap()
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();
        assert!(names.contains(&"draft_reply"));
        assert!(names.contains(&"send_reply"));
        assert!(names.contains(&"summarize_conversation"));
        assert!(names.contains(&"handoff_to_human"));
    }
}
