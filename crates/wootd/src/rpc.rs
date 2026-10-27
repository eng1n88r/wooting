//! Newline-delimited JSON-RPC 2.0 over a Unix socket.

use anyhow::Result;
use serde_json::{json, Value};
use std::{path::PathBuf, sync::Arc};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    net::{UnixListener, UnixStream},
};
use wooting_core::api::Api;

pub async fn serve_socket(api: Arc<Api>, path: PathBuf) -> Result<()> {
    if path.exists() {
        // Refuse to steal a live daemon's socket; clean up a stale one.
        if UnixStream::connect(&path).await.is_ok() {
            anyhow::bail!("another wootd is already listening on {}", path.display());
        }
        std::fs::remove_file(&path)?;
    }
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let listener = UnixListener::bind(&path)?;
    loop {
        let (stream, _) = listener.accept().await?;
        let api = api.clone();
        tokio::spawn(async move {
            if let Err(e) = handle(api, stream).await {
                tracing::debug!(error = %e, "rpc connection ended");
            }
        });
    }
}

async fn handle(api: Arc<Api>, stream: UnixStream) -> Result<()> {
    let (r, mut w) = stream.into_split();
    let mut lines = BufReader::new(r).lines();
    while let Some(line) = lines.next_line().await? {
        if line.trim().is_empty() {
            continue;
        }
        let resp = match serde_json::from_str::<Value>(&line) {
            Ok(req) => dispatch(&api, req).await,
            Err(e) => json!({"jsonrpc": "2.0", "id": null, "error": {"code": -32700, "message": e.to_string()}}),
        };
        let mut out = serde_json::to_string(&resp)?;
        out.push('\n');
        w.write_all(out.as_bytes()).await?;
    }
    Ok(())
}

/// Run one request on the blocking pool and wrap the outcome as JSON-RPC.
pub async fn dispatch(api: &Arc<Api>, req: Value) -> Value {
    let id = req.get("id").cloned().unwrap_or(Value::Null);
    let Some(method) = req.get("method").and_then(Value::as_str).map(str::to_owned) else {
        return json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32600, "message": "missing method"}});
    };
    let params = req.get("params").cloned().unwrap_or_else(|| json!({}));
    match call_blocking(api, method, params).await {
        Ok(result) => json!({"jsonrpc": "2.0", "id": id, "result": result}),
        Err(e) => json!({"jsonrpc": "2.0", "id": id, "error": e}),
    }
}

pub async fn call_blocking(
    api: &Arc<Api>,
    method: String,
    params: Value,
) -> wooting_core::api::ApiResult {
    let api = api.clone();
    tokio::task::spawn_blocking(move || api.call(&method, params))
        .await
        .unwrap_or_else(|e| {
            Err(wooting_core::api::ApiError {
                code: -32000,
                message: format!("worker panicked: {e}"),
            })
        })
}
