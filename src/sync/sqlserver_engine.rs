#[cfg(feature = "compression")]
use std::io::Write;

use anyhow::Context;
use backoff::ExponentialBackoffBuilder;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};
use tracing::{info, warn};

use crate::{
    config::{ApiConfig, SyncConfig},
    source::sqlserver::SqlServerSource,
    sync::SyncOutcome,
    sync::error::UploadError,
};

/// SQL Server `tbl_weightInfo` -> cloud synchronization engine.
///
/// Reads rows flagged `isUploadCloud = 0` via [`SqlServerSource`], uploads them as
/// a JSON batch with exponential backoff, and on success writes back
/// `isUploadCloud = 1` (when `mark_uploaded` is enabled).
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

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(default)]
struct SqlServerUploadResponse {
    accepted_serial_nos: Vec<String>,
    failed_serial_nos: Vec<String>,
}

impl SqlServerSyncEngine {
    /// Build an engine over a SQL Server source with an HTTP client tuned to the
    /// API timeout and the sync tuning parameters.
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

    /// Run a single sync cycle: fetch pending rows, upload with retry, mark uploaded.
    pub async fn sync_once(&self) -> anyhow::Result<SyncOutcome> {
        let records = self.source.fetch_pending(self.sync.batch_size).await?;
        if records.is_empty() {
            info!(
                stage = "sqlserver.fetch",
                "没有待上报的 SQL Server 称重记录"
            );
            return Ok(SyncOutcome::no_data());
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
                self.upload_batch(&payload_records)
                    .await
                    .map_err(UploadError::into_backoff)
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
        .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                return Err(error).context(
                    "SQL Server upload failed (permanent error or retry budget exhausted)",
                );
            }
        };

        let accepted_serial_nos =
            if response.accepted_serial_nos.is_empty() && response.failed_serial_nos.is_empty() {
                serial_nos.clone()
            } else {
                response.accepted_serial_nos
            };

        let marked_uploaded = if accepted_serial_nos.is_empty() {
            self.source.mark_uploaded_enabled()
        } else {
            self.source.mark_uploaded(&accepted_serial_nos).await?;
            self.source.mark_uploaded_enabled()
        };

        let outcome = SyncOutcome {
            fetched: serial_nos.len(),
            synced: accepted_serial_nos.len(),
            failed: response.failed_serial_nos.len(),
            marked_uploaded: Some(marked_uploaded),
            no_data: false,
        };

        info!(
            stage = "sqlserver.upload.done",
            ?outcome,
            "SQL Server 数据上报完成"
        );
        Ok(outcome)
    }

    async fn upload_batch(
        &self,
        records: &[Map<String, Value>],
    ) -> Result<SqlServerUploadResponse, UploadError> {
        let payload = SqlServerUploadRequest {
            source: "sqlserver-yunfu-tbl_weightInfo",
            database: self.source.database_name().to_owned(),
            table: self.source.table_name().to_owned(),
            uploaded_at: Utc::now(),
            records: records.to_vec(),
        };

        let mut request = self.http.post(&self.api.endpoint);
        if let Some(api_key) = &self.api.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = build_payload_request(request, &payload)
            .map_err(UploadError::Permanent)?
            .send()
            .await
            .context("failed to send SQL Server upload request")
            .map_err(UploadError::Transient)?;

        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(SqlServerUploadResponse::default());
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(UploadError::from_status(status, body));
        }

        let body = response
            .text()
            .await
            .context("failed to read SQL Server upload response body")
            .map_err(UploadError::Transient)?;
        if body.trim().is_empty() {
            return Ok(SqlServerUploadResponse::default());
        }

        serde_json::from_str(&body)
            .context("failed to decode SQL Server upload response")
            .map_err(UploadError::Permanent)
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
