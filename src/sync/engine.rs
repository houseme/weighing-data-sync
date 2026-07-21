#[cfg(feature = "compression")]
use std::io::Write;

use anyhow::{Context, anyhow};
use backoff::ExponentialBackoffBuilder;
use chrono::{DateTime, Utc};
use reqwest::StatusCode;
use serde::{Deserialize, Serialize};
use tracing::{info, warn};
use uuid::Uuid;

use crate::{
    config::{ApiConfig, SyncConfig},
    db::models::WeighingRecord,
    db::{self, DbPool},
};

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

#[derive(Debug, Clone, Serialize)]
pub struct SyncSummary {
    pub fetched: usize,
    pub synced: usize,
    pub failed: usize,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SyncResult {
    NoData,
    Completed(SyncSummary),
}

impl SyncEngine {
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

    pub async fn sync_once(&self) -> anyhow::Result<SyncResult> {
        let records = db::fetch_pending(&self.pool, self.sync.batch_size).await?;
        if records.is_empty() {
            info!(stage = "sync.fetch", "没有待同步称重记录");
            return Ok(SyncResult::NoData);
        }

        let record_ids = records.iter().map(|record| record.id).collect::<Vec<_>>();
        info!(
            stage = "sync.upload.begin",
            count = records.len(),
            "开始批量上传称重记录"
        );

        let retry_policy = ExponentialBackoffBuilder::new()
            .with_initial_interval(self.sync.retry_initial_delay())
            .with_max_interval(self.sync.retry_max_delay())
            .with_max_elapsed_time(Some(self.sync.retry_max_elapsed()))
            .build();

        let response = backoff::future::retry_notify(
            retry_policy,
            || async {
                self.upload_batch(&records)
                    .await
                    .map_err(backoff::Error::transient)
            },
            |error: anyhow::Error, delay: std::time::Duration| {
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
                return Err(error).context("batch upload failed after retry budget was exhausted");
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

        let summary = SyncSummary {
            fetched: records.len(),
            synced: accepted_ids.len(),
            failed: response.failed_ids.len(),
        };
        info!(stage = "sync.upload.done", ?summary, "批量同步完成");
        Ok(SyncResult::Completed(summary))
    }

    async fn upload_batch(
        &self,
        records: &[WeighingRecord],
    ) -> anyhow::Result<BatchUploadResponse> {
        let payload = BatchUploadRequest {
            source: "weighing-data-sync",
            uploaded_at: Utc::now(),
            records: records.iter().map(UploadRecord::from).collect(),
        };

        let mut request = self.http.post(&self.api.endpoint);
        if let Some(api_key) = &self.api.api_key {
            request = request.bearer_auth(api_key);
        }

        let response = build_payload_request(request, &payload)?
            .send()
            .await
            .context("failed to send upload request")?;

        let status = response.status();
        if status == StatusCode::NO_CONTENT {
            return Ok(BatchUploadResponse {
                accepted_ids: records.iter().map(|record| record.id).collect(),
                failed_ids: Vec::new(),
            });
        }

        if !status.is_success() {
            let body = response.text().await.unwrap_or_default();
            return Err(anyhow!("remote upload failed with status {status}: {body}"));
        }

        response
            .json::<BatchUploadResponse>()
            .await
            .context("failed to decode upload response")
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
