//! Async wrapper around the shared API backend for use from the GTK main loop.

use gtk::{gio, glib};
use serde_json::Value;
use std::sync::{Arc, Mutex};
use wooting_core::{
    api::ApiError,
    client::Backend as CoreBackend,
};

#[derive(Clone)]
pub struct Backend(Arc<Mutex<CoreBackend>>);

impl Backend {
    pub fn new() -> Backend {
        Backend(Arc::new(Mutex::new(CoreBackend::auto(None))))
    }

    /// Run a method on a worker thread and resolve on the main loop.
    pub async fn call(&self, method: &'static str, params: Value) -> Result<Value, ApiError> {
        let inner = self.0.clone();
        gio::spawn_blocking(move || {
            let mut b = inner.lock().unwrap_or_else(|p| p.into_inner());
            b.call(method, params)
        })
        .await
        .unwrap_or_else(|_| {
            Err(ApiError {
                code: -32000,
                message: "worker panicked".into(),
            })
        })
    }

    pub fn is_daemon(&self) -> bool {
        self.0.lock().map(|b| b.is_daemon()).unwrap_or(false)
    }
}

/// Spawn a future on the GTK main context.
pub fn spawn<F: std::future::Future<Output = ()> + 'static>(f: F) {
    glib::MainContext::default().spawn_local(f);
}
