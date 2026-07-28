use std::path::Path;

use anyhow::{Context, anyhow};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, EntityTrait, ExprTrait, QueryFilter, QueryOrder, QuerySelect, Set,
    Statement, Value, sea_query::Expr,
};
use uuid::Uuid;

use crate::{
    config::DatabaseConfig,
    db::models::WeighingRecord,
    entity::weighing_record::{self, Column, Entity as WeighingRecordEntity},
};

pub mod models;

pub type DbPool = DatabaseConnection;

/// Connect to the local SQLite cache, creating the database file and running
/// migrations (creates `weighing_records` and `inbound_payloads` if absent).
pub async fn connect(cfg: &DatabaseConfig) -> anyhow::Result<DbPool> {
    ensure_sqlite_parent_dir(&cfg.url)?;

    let mut options = ConnectOptions::new(cfg.url.clone());
    options
        .max_connections(cfg.max_connections)
        .sqlx_logging(false);

    let db = Database::connect(options)
        .await
        .context("failed to connect SQLite database with SeaORM")?;

    run_migrations(&db).await?;
    Ok(db)
}

/// Insert a single [`WeighingRecord`] into the local cache as a pending row.
pub async fn insert_record(db: &DbPool, record: &WeighingRecord) -> anyhow::Result<()> {
    let active = weighing_record::ActiveModel {
        id: Set(record.id.to_string()),
        ticket_no: Set(record.ticket_no.clone()),
        scale_no: Set(record.scale_no.clone()),
        plate_no: Set(record.plate_no.clone()),
        weight_kg: Set(record.weight_kg),
        original_unit: Set(record.original_unit.as_str().to_owned()),
        measured_at: Set(record.measured_at.to_rfc3339()),
        status: Set(record.status.as_str().to_owned()),
        retry_count: Set(record.retry_count),
        last_error: Set(record.last_error.clone()),
        created_at: Set(record.created_at.to_rfc3339()),
        updated_at: Set(record.updated_at.to_rfc3339()),
    };

    active
        .insert(db)
        .await
        .context("failed to insert weighing record with SeaORM")?;
    Ok(())
}

/// Fetch up to `limit` pending or failed records, oldest first, for upload.
pub async fn fetch_pending(db: &DbPool, limit: u32) -> anyhow::Result<Vec<WeighingRecord>> {
    let models = WeighingRecordEntity::find()
        .filter(Column::Status.is_in(["pending", "failed"]))
        .order_by_asc(Column::MeasuredAt)
        .limit(u64::from(limit))
        .all(db)
        .await
        .context("failed to fetch pending weighing records with SeaORM")?;

    models.into_iter().map(WeighingRecord::try_from).collect()
}

/// Mark the given records as successfully synced, clearing any prior error.
///
/// Issued as a single bulk `UPDATE ... WHERE id IN (...)` instead of a per-row
/// load-and-save, so a batch of N records costs one round-trip rather than N.
pub async fn mark_synced(db: &DbPool, ids: &[Uuid]) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let id_strings: Vec<String> = ids.iter().map(Uuid::to_string).collect();

    WeighingRecordEntity::update_many()
        .col_expr(Column::Status, Expr::val("synced"))
        .col_expr(Column::LastError, Expr::val(None::<String>))
        .col_expr(Column::UpdatedAt, Expr::val(now))
        .filter(Column::Id.is_in(id_strings))
        .exec(db)
        .await
        .context("failed to mark records as synced with SeaORM")?;
    Ok(())
}

/// Mark the given records as failed: increment `retry_count`, record the error.
///
/// Bulk `UPDATE ... WHERE id IN (...)` with `retry_count = retry_count + 1`.
pub async fn mark_failed(db: &DbPool, ids: &[Uuid], error: &str) -> anyhow::Result<()> {
    if ids.is_empty() {
        return Ok(());
    }
    let now = chrono::Utc::now().to_rfc3339();
    let id_strings: Vec<String> = ids.iter().map(Uuid::to_string).collect();

    WeighingRecordEntity::update_many()
        .col_expr(Column::Status, Expr::val("failed"))
        .col_expr(Column::RetryCount, Expr::col(Column::RetryCount).add(1))
        .col_expr(Column::LastError, Expr::val(error.to_owned()))
        .col_expr(Column::UpdatedAt, Expr::val(now))
        .filter(Column::Id.is_in(id_strings))
        .exec(db)
        .await
        .context("failed to mark records as failed with SeaORM")?;
    Ok(())
}

/// Persist a single received batch as raw JSON for auditing / local-first receipt.
///
/// One row per inbound HTTP request is written to the `inbound_payloads` table,
/// keyed by `request_id`. This realizes the "local-first receipt" goal without
/// forcing a mapping onto the [`WeighingRecord`] schema.
pub async fn insert_inbound_payload(
    db: &DbPool,
    request_id: &Uuid,
    source: &str,
    database: &str,
    table: &str,
    records_count: usize,
    payload: &serde_json::Value,
) -> anyhow::Result<()> {
    let payload_json =
        serde_json::to_string(payload).context("failed to serialize inbound payload")?;
    let received_at = chrono::Utc::now().to_rfc3339();
    let values: Vec<Value> = vec![
        request_id.to_string().into(),
        source.to_string().into(),
        database.to_string().into(),
        table.to_string().into(),
        (records_count as i64).into(),
        payload_json.into(),
        received_at.into(),
    ];

    let stmt = Statement::from_sql_and_values(
        DatabaseBackend::Sqlite,
        r#"INSERT INTO inbound_payloads
           (request_id, source, source_db, source_table, records_count, payload_json, received_at)
           VALUES (?, ?, ?, ?, ?, ?, ?)"#,
        values,
    );

    db.execute_raw(stmt)
        .await
        .context("failed to insert inbound payload")?;
    Ok(())
}

async fn run_migrations(db: &DbPool) -> anyhow::Result<()> {
    let backend = db.get_database_backend();
    if backend != DatabaseBackend::Sqlite {
        return Err(anyhow!(
            "local cache currently expects SQLite, got {backend:?}"
        ));
    }

    execute_sql(
        db,
        r#"
        CREATE TABLE IF NOT EXISTS weighing_records (
            id TEXT PRIMARY KEY NOT NULL,
            ticket_no TEXT NOT NULL,
            scale_no TEXT NOT NULL,
            plate_no TEXT,
            weight_kg REAL NOT NULL,
            original_unit TEXT NOT NULL,
            measured_at TEXT NOT NULL,
            status TEXT NOT NULL CHECK(status IN ('pending', 'synced', 'failed')),
            retry_count INTEGER NOT NULL DEFAULT 0,
            last_error TEXT,
            created_at TEXT NOT NULL,
            updated_at TEXT NOT NULL
        )
        "#,
    )
    .await
    .context("failed to create weighing_records table")?;

    execute_sql(
        db,
        "CREATE INDEX IF NOT EXISTS idx_weighing_records_status_time ON weighing_records(status, measured_at)",
    )
    .await
    .context("failed to create weighing_records status index")?;

    execute_sql(
        db,
        r#"
        CREATE TABLE IF NOT EXISTS inbound_payloads (
            request_id TEXT PRIMARY KEY NOT NULL,
            source TEXT NOT NULL,
            source_db TEXT NOT NULL,
            source_table TEXT NOT NULL,
            records_count INTEGER NOT NULL,
            payload_json TEXT NOT NULL,
            received_at TEXT NOT NULL
        )
        "#,
    )
    .await
    .context("failed to create inbound_payloads table")?;

    Ok(())
}

async fn execute_sql(db: &DbPool, sql: &str) -> anyhow::Result<()> {
    db.execute_unprepared(sql).await?;
    Ok(())
}

fn ensure_sqlite_parent_dir(url: &str) -> anyhow::Result<()> {
    if url == "sqlite::memory:" || url.contains(":memory:") {
        return Ok(());
    }

    let path = sqlite_path(url);
    if let Some(parent) = path.as_deref().and_then(|path| Path::new(path).parent())
        && !parent.as_os_str().is_empty()
    {
        std::fs::create_dir_all(parent)
            .with_context(|| format!("failed to create database directory {}", parent.display()))?;
    }

    if let Some(path) = path.filter(|path| path != ":memory:") {
        std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .with_context(|| format!("failed to create SQLite database file {path}"))?;
    }
    Ok(())
}

fn sqlite_path(url: &str) -> Option<String> {
    let without_query = url.split_once('?').map_or(url, |(path, _)| path);
    without_query
        .strip_prefix("sqlite://")
        .or_else(|| without_query.strip_prefix("sqlite:"))
        .filter(|path| !path.is_empty())
        .map(ToOwned::to_owned)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::DatabaseConfig;
    use crate::db::models::SyncStatus;

    fn mem_cfg() -> DatabaseConfig {
        DatabaseConfig {
            url: "sqlite::memory:".to_owned(),
            max_connections: 1,
        }
    }

    #[tokio::test]
    async fn insert_fetch_mark_round_trip() {
        let db = connect(&mem_cfg()).await.expect("connect in-memory sqlite");
        let now = chrono::Utc::now();

        let synced =
            WeighingRecord::new_kg("T1", "S1", Some("皖A12345".to_owned()), 1234.5, now).unwrap();
        let synced_id = synced.id;
        insert_record(&db, &synced).await.unwrap();

        let failed = WeighingRecord::new_kg("T2", "S1", None, 50.0, now).unwrap();
        let failed_id = failed.id;
        insert_record(&db, &failed).await.unwrap();

        let pending = fetch_pending(&db, 10).await.unwrap();
        assert_eq!(pending.len(), 2);
        assert_eq!(pending[0].status, SyncStatus::Pending);

        // Bulk mark_synced removes the record from the pending/failed set.
        mark_synced(&db, &[synced_id]).await.unwrap();
        let pending = fetch_pending(&db, 10).await.unwrap();
        assert_eq!(pending.len(), 1);
        assert!(pending.iter().all(|r| r.id != synced_id));

        // Bulk mark_failed increments retry_count and records the error.
        mark_failed(&db, &[failed_id], "boom").await.unwrap();
        let pending = fetch_pending(&db, 10).await.unwrap();
        let failed_row = pending.iter().find(|r| r.id == failed_id).unwrap();
        assert_eq!(failed_row.status, SyncStatus::Failed);
        assert_eq!(failed_row.retry_count, 1);
        assert_eq!(failed_row.last_error.as_deref(), Some("boom"));

        // Idempotency guards: empty id slices are no-ops.
        mark_synced(&db, &[]).await.unwrap();
        mark_failed(&db, &[], "x").await.unwrap();
    }
}
