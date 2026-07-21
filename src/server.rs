use std::{net::SocketAddr, time::Instant};

use anyhow::Context;
use axum::{
    Json, Router,
    extract::DefaultBodyLimit,
    http::StatusCode,
    response::IntoResponse,
    routing::{get, post},
};
use chrono::Utc;
use serde::Serialize;
use serde_json::{Value, json};
use tracing::{error, info, warn};
use uuid::Uuid;

use crate::config::ServerConfig;

#[derive(Debug, Serialize)]
struct PutResponse {
    request_id: Uuid,
    accepted: bool,
    accepted_serial_nos: Vec<String>,
    failed_serial_nos: Vec<String>,
    records_count: usize,
    received_at: String,
}

pub async fn run_http_server(cfg: ServerConfig) -> anyhow::Result<()> {
    let bind: SocketAddr = cfg
        .bind
        .parse()
        .with_context(|| format!("invalid server bind address: {}", cfg.bind))?;

    let app = Router::new()
        .route("/", get(health))
        .route(cfg.route.as_str(), post(put_weighing_data))
        .layer(DefaultBodyLimit::max(cfg.max_body_bytes));

    let listener = tokio::net::TcpListener::bind(bind)
        .await
        .with_context(|| format!("failed to bind HTTP server on {bind}"))?;

    info!(
        stage = "server.ready",
        bind = %bind,
        route = %cfg.route,
        max_body_bytes = cfg.max_body_bytes,
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

async fn put_weighing_data(Json(payload): Json<Value>) -> impl IntoResponse {
    let started_at = Instant::now();
    let request_id = Uuid::new_v4();
    let records = payload
        .get("records")
        .and_then(Value::as_array)
        .cloned()
        .unwrap_or_default();
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

    if records.is_empty() {
        warn!(
            stage = "server.put",
            %request_id,
            payload = %payload,
            "收到称重数据上报，但 records 为空或不存在"
        );
    } else {
        let source = payload
            .get("source")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let database = payload
            .get("database")
            .and_then(serde_json::Value::as_str)
            .unwrap_or("unknown");
        let table = payload
            .get("table")
            .and_then(serde_json::Value::as_str)
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
    }

    let response = PutResponse {
        request_id,
        accepted: true,
        accepted_serial_nos,
        failed_serial_nos: Vec::new(),
        records_count: records.len(),
        received_at: Utc::now().to_rfc3339(),
    };

    (StatusCode::OK, Json(response))
}

async fn shutdown_signal() {
    if let Err(error) = tokio::signal::ctrl_c().await {
        error!(stage = "server.shutdown", %error, "等待停止信号失败");
    }
    info!(stage = "server.shutdown", "称重数据接收服务正在关闭");
}
