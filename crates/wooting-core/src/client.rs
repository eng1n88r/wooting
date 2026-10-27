//! Blocking client for the daemon's Unix-socket JSON-RPC endpoint.

use crate::api::{ApiError, ApiResult};
use serde_json::{json, Value};
use std::{
    io::{BufRead, BufReader, Write},
    os::unix::net::UnixStream,
    path::{Path, PathBuf},
    time::Duration,
};

/// `$XDG_RUNTIME_DIR/wootd.sock`, falling back to `/tmp/wootd-<uid>.sock`.
pub fn default_socket_path() -> PathBuf {
    if let Some(dir) = dirs::runtime_dir() {
        return dir.join("wootd.sock");
    }
    let uid = std::fs::metadata("/proc/self").map(|m| {
        use std::os::unix::fs::MetadataExt;
        m.uid()
    });
    PathBuf::from(format!("/tmp/wootd-{}.sock", uid.unwrap_or(0)))
}

pub struct Client {
    stream: BufReader<UnixStream>,
    next_id: u64,
}

impl Client {
    pub fn connect(path: &Path) -> std::io::Result<Client> {
        let s = UnixStream::connect(path)?;
        s.set_read_timeout(Some(Duration::from_secs(30)))?;
        Ok(Client {
            stream: BufReader::new(s),
            next_id: 1,
        })
    }

    pub fn call(&mut self, method: &str, params: Value) -> ApiResult {
        let id = self.next_id;
        self.next_id += 1;
        let req = json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params});
        let mut line = serde_json::to_string(&req).map_err(|e| ApiError {
            code: -32700,
            message: e.to_string(),
        })?;
        line.push('\n');
        let io_err = |e: std::io::Error| ApiError {
            code: -32002,
            message: format!("daemon connection: {e}"),
        };
        self.stream
            .get_mut()
            .write_all(line.as_bytes())
            .map_err(io_err)?;
        let mut resp = String::new();
        self.stream.read_line(&mut resp).map_err(io_err)?;
        if resp.is_empty() {
            return Err(ApiError {
                code: -32002,
                message: "daemon closed the connection".into(),
            });
        }
        let v: Value = serde_json::from_str(&resp).map_err(|e| ApiError {
            code: -32700,
            message: format!("bad response from daemon: {e}"),
        })?;
        if let Some(err) = v.get("error") {
            return Err(serde_json::from_value(err.clone()).unwrap_or(ApiError {
                code: -32000,
                message: err.to_string(),
            }));
        }
        Ok(v.get("result").cloned().unwrap_or(Value::Null))
    }
}

/// Something that can run API methods: the daemon over its socket, or a
/// local [`crate::api::Api`] when no daemon is running.
pub enum Backend {
    Daemon(Client),
    Local(Box<crate::api::Api>),
}

impl Backend {
    /// Prefer the daemon when its socket accepts connections.
    pub fn auto(selector: Option<String>) -> Backend {
        match Client::connect(&default_socket_path()) {
            Ok(c) => Backend::Daemon(c),
            Err(_) => Backend::Local(Box::new(crate::api::Api::new(selector))),
        }
    }

    pub fn call(&mut self, method: &str, params: Value) -> ApiResult {
        match self {
            Backend::Daemon(c) => c.call(method, params),
            Backend::Local(api) => api.call(method, params),
        }
    }

    pub fn is_daemon(&self) -> bool {
        matches!(self, Backend::Daemon(_))
    }
}
