//! Cloud synchronization engines and shared types.
//!
//! Two engines share this module:
//! - [`engine::SyncEngine`] reads pending rows from the local SQLite cache
//!   ([`crate::db`]) and uploads them, marking rows `synced` / `failed`.
//! - [`sqlserver_engine::SqlServerSyncEngine`] reads `tbl_weightInfo` rows from
//!   SQL Server via [`crate::source::sqlserver`], uploads them, and writes back
//!   `isUploadCloud = 1` on success.
//!
//! Both return a type-erased [`SyncOutcome`] so the CLI can log a uniform summary.

pub mod engine;
pub mod error;
pub mod sqlserver_engine;

use serde::Serialize;

/// Type-erased summary of a single sync cycle, shared by both sync engines.
#[derive(Debug, Clone, Serialize)]
pub struct SyncOutcome {
    /// Records pulled from the source for this cycle.
    pub fetched: usize,
    /// Records the cloud endpoint acknowledged.
    pub synced: usize,
    /// Records the cloud endpoint rejected.
    pub failed: usize,
    /// Whether the source-of-truth was marked as already uploaded (SQL Server path only).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub marked_uploaded: Option<bool>,
    /// `true` when the source had nothing to sync.
    pub no_data: bool,
}

impl SyncOutcome {
    /// Build a `no_data` outcome (nothing to sync).
    pub fn no_data() -> Self {
        Self {
            fetched: 0,
            synced: 0,
            failed: 0,
            marked_uploaded: None,
            no_data: true,
        }
    }
}
