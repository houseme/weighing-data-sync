use std::path::Path;

use anyhow::{Context, anyhow};
use sea_orm::{
    ActiveModelTrait, ColumnTrait, ConnectOptions, ConnectionTrait, Database, DatabaseBackend,
    DatabaseConnection, EntityTrait, IntoActiveModel, QueryFilter, QueryOrder, QuerySelect, Set,
};
use uuid::Uuid;

use crate::{
    config::DatabaseConfig,
    db::models::WeighingRecord,
    entity::weighing_record::{self, Column, Entity as WeighingRecordEntity},
};

pub mod models;

pub type DbPool = DatabaseConnection;

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

pub async fn mark_synced(db: &DbPool, ids: &[Uuid]) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    for id in ids {
        let model = WeighingRecordEntity::find_by_id(id.to_string())
            .one(db)
            .await
            .with_context(|| format!("failed to load record {id} for synced update"))?
            .with_context(|| format!("weighing record {id} not found"))?;

        let mut active = model.into_active_model();
        active.status = Set("synced".to_owned());
        active.last_error = Set(None);
        active.updated_at = Set(now.clone());
        active
            .update(db)
            .await
            .with_context(|| format!("failed to mark record {id} as synced with SeaORM"))?;
    }
    Ok(())
}

pub async fn mark_failed(db: &DbPool, ids: &[Uuid], error: &str) -> anyhow::Result<()> {
    let now = chrono::Utc::now().to_rfc3339();

    for id in ids {
        let model = WeighingRecordEntity::find_by_id(id.to_string())
            .one(db)
            .await
            .with_context(|| format!("failed to load record {id} for failed update"))?
            .with_context(|| format!("weighing record {id} not found"))?;

        let retry_count = model.retry_count;
        let mut active = model.into_active_model();
        active.status = Set("failed".to_owned());
        active.retry_count = Set(retry_count + 1);
        active.last_error = Set(Some(error.to_owned()));
        active.updated_at = Set(now.clone());
        active
            .update(db)
            .await
            .with_context(|| format!("failed to mark record {id} as failed with SeaORM"))?;
    }
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
    if let Some(parent) = path.as_deref().and_then(|path| Path::new(path).parent()) {
        if !parent.as_os_str().is_empty() {
            std::fs::create_dir_all(parent).with_context(|| {
                format!("failed to create database directory {}", parent.display())
            })?;
        }
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
