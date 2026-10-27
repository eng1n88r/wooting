//! MCP server on stdio. Tools are generated from the shared method table.

use anyhow::Result;
use rmcp::{
    model::{
        CallToolRequestParams, CallToolResponse, CallToolResult, ContentBlock, Implementation,
        ListToolsResult, PaginatedRequestParams, ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
    ErrorData, RoleServer, ServerHandler, ServiceExt,
};
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::sync::Mutex;
use wooting_core::{
    api::{methods, Api},
    client::{Backend, Client},
};

/// Method names use dots; MCP tool names must match `[a-zA-Z0-9_-]`.
fn tool_name(method: &str) -> String {
    method.replace('.', "_")
}

/// Reverse of [`tool_name`], resolved against the method table because
/// method names may themselves contain underscores (`rgb.set_key`).
fn method_name(tool: &str) -> Option<&'static str> {
    methods()
        .into_iter()
        .map(|m| m.name)
        .find(|m| tool_name(m) == tool)
}

#[derive(Clone)]
struct WootingMcp {
    backend: Arc<Mutex<Backend>>,
}

impl ServerHandler for WootingMcp {
    fn get_info(&self) -> ServerConfig {
        ServerConfig::new(ServerCapabilities::builder().enable_tools().build())
            .with_server_info(Implementation::new("wootd", env!("CARGO_PKG_VERSION")))
            .with_instructions(
                "Controls a Wooting keyboard attached to this machine. Use device_keys to \
                 learn key ids, then rgb_set_keys / rgb_fill / rgb_from_image to light it. \
                 Direct lighting is volatile; rgb_save persists it. rgb_release hands the \
                 LEDs back to the keyboard.",
            )
    }

    async fn list_tools(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, ErrorData> {
        let tools = methods()
            .into_iter()
            .map(|m| {
                let schema = m.params.as_object().cloned().unwrap_or_default();
                Tool::new(tool_name(m.name), m.description, schema)
            })
            .collect();
        Ok(ListToolsResult::with_all_items(tools))
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, ErrorData> {
        let Some(method) = method_name(&request.name) else {
            return Ok(CallToolResult::error(vec![ContentBlock::text(format!(
                "unknown tool {}",
                request.name
            ))])
            .into());
        };
        let params = request
            .arguments
            .map(Value::Object)
            .unwrap_or_else(|| json!({}));
        let backend = self.backend.clone();
        let result = tokio::task::spawn_blocking(move || {
            let mut b = backend.blocking_lock();
            b.call(method, params)
        })
        .await
        .map_err(|e| ErrorData::internal_error(e.to_string(), None))?;
        let res = match result {
            Ok(v) => CallToolResult::success(vec![ContentBlock::text(
                serde_json::to_string_pretty(&v).unwrap_or_default(),
            )]),
            Err(e) => CallToolResult::error(vec![ContentBlock::text(e.message)]),
        };
        Ok(res.into())
    }
}

pub async fn serve_stdio(socket: PathBuf, device: Option<String>) -> Result<()> {
    let backend = match Client::connect(&socket) {
        Ok(c) => {
            tracing::info!(socket = %socket.display(), "forwarding to daemon");
            Backend::Daemon(c)
        }
        Err(_) => {
            tracing::info!("no daemon socket; opening keyboard directly");
            Backend::Local(Box::new(Api::new(device)))
        }
    };
    let server = WootingMcp {
        backend: Arc::new(Mutex::new(backend)),
    };
    let service = server.serve(rmcp::transport::stdio()).await?;
    service.waiting().await?;
    Ok(())
}
