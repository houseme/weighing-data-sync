use std::{net::SocketAddr, time::Instant};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::{DefaultBodyLimit, State},
    http::{HeaderMap, StatusCode},
    response::{IntoResponse, Response},
    routing::{get, post},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::ServerConfig;
use crate::db::{self, DbPool};

/// Shared state for the receive HTTP server: an optional bearer token and an
/// optional SQLite handle for local-first persistence of received batches.
#[derive(Clone)]
struct AppState {
    api_key: Option<String>,
    db: Option<DbPool>,
}

#[derive(Debug, Serialize)]
struct PutResponse {
    request_id: Uuid,
    accepted: bool,
    accepted_serial_nos: Vec<String>,
    failed_serial_nos: Vec<String>,
    records_count: usize,
    received_at: String,
}

/// Run the receive HTTP server.
///
/// When `db` is `Some`, each accepted batch is also persisted as raw JSON into
/// the local `inbound_payloads` table. When `cfg.api_key` is set, the receive
/// route requires a matching `Authorization: Bearer <token>` header.
pub async fn run_http_server(cfg: ServerConfig, db: Option<DbPool>) -> anyhow::Result<()> {
    let bind: SocketAddr = cfg
        .bind
        .parse()
        .with_context(|| format!("invalid server bind address: {}", cfg.bind))?;

    let auth_enabled = cfg.api_key.is_some();
    let persist_enabled = db.is_some();
    let state = AppState {
        api_key: cfg.api_key,
        db,
    };

    let app = Router::new()
        .route("/", get(health))
        .route(cfg.route.as_str(), post(put_weighing_data))
        .layer(DefaultBodyLimit::max(cfg.max_body_bytes))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HTTP server on {bind}"))?;

    info!(
        stage = "server.ready",
        %bind,
        route = %cfg.route,
        max_body_bytes = cfg.max_body_bytes,
        auth_enabled,
        persist_enabled,
        "称重数据接收服务已启动"
    );

    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await
        .context("HTTP server failed")
}

async fn health() -> Json<Value> {
    Json(json!({
        "service": "weighing-data-sync",
        "status": "ok",
        "time": Utc::now().to_rfc3339(),
    }))
}

async fn put_weighing_data(
    State(state): State<AppState>,
    headers: HeaderMap,
    Json(payload): Json<Value>,
) -> Response {
    let started_at = Instant::now();
    let request_id = Uuid::new_v4();

    if let Some(expected) = &state.api_key {
        match extract_bearer_token(&headers) {
            Some(token) if token.trim() == expected => {}
            _ => {
                warn!(
                    stage = "server.put",
                    %request_id,
                    "收到未授权的称重数据上报，已拒绝"
                );
                return (
                    StatusCode::UNAUTHORIZED,
                    Json(json!({ "request_id": request_id, "error": "invalid or missing bearer token" })),
                )
                    .into_response();
            }
        }
    }

    let records = payload
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();

    if records.is_empty() {
        warn!(
            stage = "server.put",
            %request_id,
            payload = %payload,
            "收到称重数据上报，但 records 为空或不存在"
        );
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "request_id": request_id, "error": "records is missing or empty" })),
        )
            .into_response();
    }

    let accepted_serial_nos = records
        .iter()
        .filter_map(|record| {
            record
                .get("serialNo")
                .or_else(|| record.get("serial_no"))
                .and_then(Value::as_str)
                .map(ToOwned::to_owned)
        })
        .collect::<Vec<_>>();

    let source = payload
        .get("source")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let database = payload
        .get("database")
        .and_then(Value::as_str)
        .unwrap_or("unknown");
    let table = payload
        .get("table")
        .and_then(Value::as_str)
        .unwrap_or("unknown");

    info!(
        stage = "server.put",
        %request_id,
        source,
        database,
        table,
        records_count = records.len(),
        accepted_serial_nos = ?accepted_serial_nos,
        elapsed_ms = started_at.elapsed().as_millis(),
        payload = %payload,
        "收到称重数据上报"
    );

    // The receiver accepts every well-formed record; it never computes a
    // per-record reject set, so `failed_serial_nos` stays empty by design.
    // When persistence is enabled, the full payload is preserved for replay.
    if let Some(db) = &state.db
        && let Err(error) = db::insert_inbound_payload(
            db,
            &request_id,
            source,
            database,
            table,
            records.len(),
            &payload,
        )
        .await
    {
        warn!(
            stage = "server.put",
            %request_id,
            %error,
            "接收数据落库失败，仅记录日志"
        );
    }

    let response = PutResponse {
        request_id,
        accepted: true,
        accepted_serial_nos,
        failed_serial_nos: Vec::new(),
        records_count: records.len(),
        received_at: Utc::now().to_rfc3339(),
    };

    (StatusCode::OK, Json(response)).into_response()
}

/// Extract a bearer token from the `Authorization` header (case-insensitive scheme).
fn extract_bearer_token(headers: &HeaderMap) -> Option<&str> {
    let value = headers
        .get(axum::http::header::AUTHORIZATION)?
        .to_str()
        .ok()?;
    let value = value.trim();
    value
        .strip_prefix("Bearer ")
        .or_else(|| value.strip_prefix("bearer "))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        error!(stage = "server.shutdown", %error, "等待停止信号失败");
    }
    info!(stage = "server.shutdown", "称重数据接收服务正在关闭");
}
