#[cfg(feature = "compression")]
use std::io::Write;

use anyhow::Context;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::{ApiConfig, SyncConfig},
    db::{self, DbPool, models::WeighingRecord},
    sync::SyncOutcome,
    sync::error::UploadError,
    sync::retry::retry_transient,
};

/// Local SQLite cache -> cloud synchronization engine.
///
/// Reads pending rows from the local [`crate::db`] cache, uploads them as a JSON
/// batch to the cloud endpoint with exponential backoff, and marks each row
/// `synced` or `failed` depending on the response.
#[derive(Debug, Clone)]
pub struct SyncEngine {
    pool: DbPool,
    http: reqwest::Client,
    api: ApiConfig,
    sync: SyncConfig,
}

#[derive(Debug, Clone, Serialize)]
struct BatchUploadRequest {
    source: &'static str,
    uploaded_at: DateTime<Utc>,
    records: Vec<UploadRecord>,
}

#[derive(Debug, Clone, Serialize)]
struct UploadRecord {
    id: Uuid,
    ticket_no: String,
    scale_no: String,
    plate_no: Option<String>,
    weight_kg: f64,
    weight_lb: f64,
    original_unit: String,
    measured_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Deserialize)]
struct BatchUploadResponse {
    #[serde(default)]
    accepted_ids: Vec<Uuid>,
    #[serde(default)]
    failed_ids: Vec<Uuid>,
}

impl SyncEngine {
    /// Build an engine bound to a SQLite pool, an HTTP client tuned to the API
    /// timeout, and the sync tuning parameters.
    pub fn new(pool: DbPool, api: ApiConfig, sync: SyncConfig) -> anyhow::Result<Self> {
        let http = reqwest::Client::builder()
            .timeout(api.timeout())
            .build()
            .context("failed to build HTTP client")?;

        Ok(Self {
            pool,
            http,
            api,
            sync,
        })
    }

    /// Run a single sync cycle: fetch pending, upload with retry, mark results.
    pub async fn sync_once(&self) -> anyhow::Result<SyncOutcome> {
        let records = db::fetch_pending(&self.pool, self.sync.batch_size).await?;
        if records.is_empty() {
            info!(stage = "sync.fetch", "没有待同步称重记录");
            return Ok(SyncOutcome::no_data());
        }

        let record_ids = records.iter().map(|record| record.id).collect::<Vec<_>>();
        info!(
            stage = "sync.upload.begin",
            count = records.len(),
            "开始批量上传称重记录"
        );

        let response = retry_transient(
            self.sync.retry_initial_delay(),
            self.sync.retry_max_delay(),
            self.sync.retry_max_elapsed(),
            || async { self.upload_batch(&records).await },
            |error, delay| {
                warn!(
                    stage = "sync.retry",
                    %error,
                    retry_after_ms = delay.as_millis(),
                    "上传失败，准备按指数退避重试"
                );
            },
        )
        .await;

        let response = match response {
            Ok(response) => response,
            Err(error) => {
                db::mark_failed(&self.pool, &record_ids, &error.to_string()).await?;
                return Err(error)
                    .context("batch upload failed (permanent error or retry budget exhausted)");
            }
        };

        let accepted_ids = if response.accepted_ids.is_empty() && response.failed_ids.is_empty() {
            record_ids.clone()
        } else {
            response.accepted_ids
        };

        if !accepted_ids.is_empty() {
            db::mark_synced(&self.pool, &accepted_ids).await?;
        }

        if !response.failed_ids.is_empty() {
            db::mark_failed(
                &self.pool,
                &response.failed_ids,
                "remote endpoint rejected record",
            )
            .await?;
        }

        let outcome = SyncOutcome {
            fetched: records.len(),
            synced: accepted_ids.len(),
            failed: response.failed_ids.len(),
            marked_uploaded: None,
            no_data: false,
        };
        info!(stage = "sync.upload.done", ?outcome, "批量同步完成");
        Ok(outcome)
    }

    async fn upload_batch(
        &self,
        records: &[WeighingRecord],
    ) -> Result<BatchUploadResponse, UploadError> {
        let payload = BatchUploadRequest {
            source: "weighing-data-sync",
            uploaded_at: Utc::now(),
            records: records.iter().map(UploadRecord::from).collect(),
        };

        let mut request = self.http.post(&self.api.endpoint);
        if let Some(api_key) = &self.api.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = build_payload_request(request, &payload)
            .map_err(UploadError::Permanent)?
            .send()
            .await
            .context("failed to send upload request")
            .map_err(UploadError::Transient)?;

        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(BatchUploadResponse {
                accepted_ids: records.iter().map(|record| record.id).collect(),
                failed_ids: Vec::new(),
            });
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(UploadError::from_status(status, body));
        }

        response
            .json::<BatchUploadResponse>()
            .await
            .context("failed to decode upload response")
            .map_err(UploadError::Permanent)
    }
}

impl From<&WeighingRecord> for UploadRecord {
    fn from(record: &WeighingRecord) -> Self {
        Self {
            id: record.id,
            ticket_no: record.ticket_no.clone(),
            scale_no: record.scale_no.clone(),
            plate_no: record.plate_no.clone(),
            weight_kg: record.weight_kg,
            weight_lb: record.weight_lb(),
            original_unit: record.original_unit.as_str().to_owned(),
            measured_at: record.measured_at,
        }
    }
}

#[cfg(not(feature = "compression"))]
fn build_payload_request(
    request: reqwest::RequestBuilder,
    payload: &BatchUploadRequest,
) -> anyhow::Result<reqwest::RequestBuilder> {
    Ok(request.json(payload))
}

#[cfg(feature = "compression")]
fn build_payload_request(
    request: reqwest::RequestBuilder,
    payload: &BatchUploadRequest,
) -> anyhow::Result<reqwest::RequestBuilder> {
    let json = serde_json::to_vec(payload).context("failed to serialize upload payload")?;
    let mut encoder = flate2::write::GzEncoder::new(Vec::new(), flate2::Compression::default());
    encoder
        .write_all(&json)
        .context("failed to gzip upload payload")?;
    let compressed = encoder
        .finish()
        .context("failed to finalize gzip payload")?;

    Ok(request
        .header(reqwest::header::CONTENT_ENCODING, "gzip")
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(compressed))
}
