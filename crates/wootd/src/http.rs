//! Localhost HTTP front end. Every method is reachable as
//! `POST /v1/<method with dots replaced by slashes>` with a JSON body, or
//! via the generic `POST /v1/rpc` JSON-RPC endpoint. `GET /v1/methods`
//! describes the API; `GET /openapi.json` is a minimal OpenAPI document.

use crate::rpc;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde_json::{json, Value};
use std::{net::SocketAddr, sync::Arc};
use tower_http::cors::CorsLayer;
use wooting_core::api::{methods, Api, ApiError};

pub async fn serve(api: Arc<Api>, addr: SocketAddr) -> anyhow::Result<()> {
    let app = Router::new()
        .route("/", get(index))
        .route("/openapi.json", get(openapi))
        .route("/v1/methods", get(list_methods))
        .route("/v1/rpc", post(rpc_endpoint))
        .route(
            "/v1/{*method}",
            post(method_endpoint).get(method_endpoint_get),
        )
        .layer(CorsLayer::permissive())
        .with_state(api);
    let listener = tokio::net::TcpListener::bind(addr).await?;
    axum::serve(listener, app).await?;
    Ok(())
}

async fn index() -> Json<Value> {
    Json(json!({
        "name": "wootd",
        "version": env!("CARGO_PKG_VERSION"),
        "docs": "/v1/methods",
        "openapi": "/openapi.json",
        "example": "curl -X POST localhost:7357/v1/rgb/fill -H 'content-type: application/json' -d '{\"color\":\"#00ff00\"}'",
    }))
}

async fn list_methods() -> Json<Value> {
    Json(json!({"methods": methods()}))
}

async fn openapi() -> Json<Value> {
    let mut paths = serde_json::Map::new();
    for m in methods() {
        let path = format!("/v1/{}", m.name.replace('.', "/"));
        paths.insert(path, json!({
            "post": {
                "operationId": m.name,
                "summary": m.description,
                "requestBody": {"content": {"application/json": {"schema": m.params}}},
                "responses": {"200": {"description": "result"}, "400": {"description": "error"}},
            }
        }));
    }
    Json(json!({
        "openapi": "3.1.0",
        "info": {"title": "wootd", "version": env!("CARGO_PKG_VERSION")},
        "paths": paths,
    }))
}

struct ApiResponse(Result<Value, ApiError>);

impl IntoResponse for ApiResponse {
    fn into_response(self) -> Response {
        match self.0 {
            Ok(v) => Json(v).into_response(),
            Err(e) => {
                let status = match e.code {
                    -32601 => StatusCode::NOT_FOUND,
                    -32602 | -32700 => StatusCode::BAD_REQUEST,
                    -32001 => StatusCode::SERVICE_UNAVAILABLE,
                    _ => StatusCode::INTERNAL_SERVER_ERROR,
                };
                (status, Json(json!({"error": e}))).into_response()
            }
        }
    }
}

async fn rpc_endpoint(State(api): State<Arc<Api>>, Json(req): Json<Value>) -> Json<Value> {
    Json(rpc::dispatch(&api, req).await)
}

async fn method_endpoint(
    State(api): State<Arc<Api>>,
    Path(method): Path<String>,
    body: Option<Json<Value>>,
) -> ApiResponse {
    let params = body.map(|Json(v)| v).unwrap_or_else(|| json!({}));
    let method = method.replace('/', ".");
    if method.starts_with("links.") {
        return ApiResponse(rpc::links_call(&method, &params).map_err(|e| ApiError {
            code: -32602,
            message: e.to_string(),
        }));
    }
    ApiResponse(rpc::call_blocking(&api, method, params).await)
}

async fn method_endpoint_get(
    State(api): State<Arc<Api>>,
    Path(method): Path<String>,
) -> ApiResponse {
    ApiResponse(rpc::call_blocking(&api, method.replace('/', "."), json!({})).await)
}
