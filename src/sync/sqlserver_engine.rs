#[cfg(feature = "compression")]
use std::io::Write;

use anyhow::{Context, anyhow};
use backoff::ExponentialBackoffBuilder;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::{
    config::{ApiConfig, SyncConfig},
    source::sqlserver::SqlServerSource,
};

#[derive(Debug, Clone)]
pub struct SqlServerSyncEngine {
    http: reqwest::Client,
    api: ApiConfig,
    sync: SyncConfig,
    source: SqlServerSource,
}

#[derive(Debug, Clone, Serialize)]
struct SqlServerUploadRequest {
    source: &'static str,
    database: String,
    table: String,
    uploaded_at: DateTime<Utc>,
    records: Vec<Map<String, Value>>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
struct SqlServerUploadResponse {
    accepted_serial_nos: Vec<String>,
    failed_serial_nos: Vec<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct SqlServerSyncSummary {
    pub fetched: usize,
    pub uploaded: usize,
    pub failed: usize,
    pub marked_uploaded: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SqlServerSyncResult {
    NoData,
    Completed(SqlServerSyncSummary),
}

impl SqlServerSyncEngine {
    pub fn new(api: ApiConfig, sync: SyncConfig, source: SqlServerSource) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(api.timeout())
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            http,
            api,
            sync,
            source,
        })
    }

    pub async fn sync_once(&self) -> anyhow::Result<SqlServerSyncResult> {
        let records = self.source.fetch_pending(self.sync.batch_size).await?;
        if records.is_empty() {
            info!(
                stage = "sqlserver.fetch",
                "没有待上报的 SQL Server 称重记录"
            );
            return Ok(SqlServerSyncResult::NoData);
        }

        let serial_nos = records
            .iter()
            .map(|record| record.serial_no.clone())
            .collect::<Vec<_>>();
        let payload_records = records
            .into_iter()
            .map(|record| record.data)
            .collect::<Vec<_>>();

        info!(
            stage = "sqlserver.upload.begin",
            count = payload_records.len(),
            "开始上报 SQL Server tbl_weightInfo 数据"
        );

        let retry_policy = ExponentialBackoffBuilder::new()
            .with_initial_interval(self.sync.retry_initial_delay())
            .with_max_interval(self.sync.retry_max_delay())
            .with_max_elapsed_time(Some(self.sync.retry_max_elapsed()))
            .build();

        let response = backoff::future::retry_notify(
            retry_policy,
            || async {
                self.upload_batch(payload_records.clone())
                    .await
                    .map_err(backoff::Error::transient)
            },
            |error: anyhow::Error, delay: std::time::Duration| {
                warn!(
                    stage = "sqlserver.retry",
                    %error,
                    retry_after_ms = delay.as_millis(),
                    "SQL Server 数据上报失败，准备按指数退避重试"
                );
            },
        )
        .await
        .context("SQL Server batch upload failed after retry budget was exhausted")?;

        let accepted_serial_nos =
            if response.accepted_serial_nos.is_empty() && response.failed_serial_nos.is_empty() {
                serial_nos.clone()
            } else {
                response.accepted_serial_nos
            };

        if !accepted_serial_nos.is_empty() {
            self.source.mark_uploaded(&accepted_serial_nos).await?;
        }

        let summary = SqlServerSyncSummary {
            fetched: serial_nos.len(),
            uploaded: accepted_serial_nos.len(),
            failed: response.failed_serial_nos.len(),
            marked_uploaded: self.source.mark_uploaded_enabled(),
        };

        info!(
            stage = "sqlserver.upload.done",
            ?summary,
            "SQL Server 数据上报完成"
        );
        Ok(SqlServerSyncResult::Completed(summary))
    }

    async fn upload_batch(
        &self,
        records: Vec<Map<String, Value>>,
    ) -> anyhow::Result<SqlServerUploadResponse> {
        let payload = SqlServerUploadRequest {
            source: "sqlserver-yunfu-tbl_weightInfo",
            database: self.source.database_name().to_owned(),
            table: self.source.table_name().to_owned(),
            uploaded_at: Utc::now(),
            records,
        };

        let mut request = self.http.post(&self.api.endpoint);
        if let Some(api_key) = &self.api.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = build_payload_request(request, &payload)?
            .send()
            .await
            .context("failed to send SQL Server upload request")?;

        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(SqlServerUploadResponse::default());
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!(
                "remote SQL Server upload failed with status {status}: {body}"
            ));
        }

        let body = response
            .text()
            .await
            .context("failed to read SQL Server upload response body")?;
        if body.trim().is_empty() {
            return Ok(SqlServerUploadResponse::default());
        }

        serde_json::from_str(&body).context("failed to decode SQL Server upload response")
    }
}

impl Default for SqlServerUploadResponse {
    fn default() -> Self {
        Self {
            accepted_serial_nos: Vec::new(),
            failed_serial_nos: Vec::new(),
        }
    }
}

#[cfg(not(feature = "compression"))]
fn build_payload_request(
    request: reqwest::RequestBuilder,
    payload: &SqlServerUploadRequest,
) -> anyhow::Result<reqwest::RequestBuilder> {
    Ok(request.json(payload))
}

#[cfg(feature = "compression")]
fn build_payload_request(
    request: reqwest::RequestBuilder,
    payload: &SqlServerUploadRequest,
) -> anyhow::Result<reqwest::RequestBuilder> {
    let json =
        serde_json::to_vec(payload).context("failed to serialize SQL Server upload payload")?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&json)
        .context("failed to gzip SQL Server upload payload")?;
    let compressed = encoder
        .finish()
        .context("failed to finalize SQL Server gzip payload")?;

    Ok(request
        .header(reqwest::header::CONTENT_ENCODING, "gzip")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(compressed))
}
