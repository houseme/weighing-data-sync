use std::sync::Arc;

use anyhow::Context;
use clap::{Parser, Subcommand};
use tokio::sync::Mutex;
use tracing::{error, info, warn};
use tracing_subscriber::EnvFilter;
use weighing_data_sync::{
    config::AppConfig,
    db, server,
    source::sqlserver::SqlServerSource,
    sync::{SyncOutcome, engine::SyncEngine, sqlserver_engine::SqlServerSyncEngine},
    windows,
};

#[derive(Debug, Parser)]
#[command(name = "sync-daemon")]
#[command(about = "Shangheng weighing data synchronization daemon")]
struct Cli {
    #[arg(
        short,
        long,
        env = "WDS_CONFIG",
        default_value = "config/default.toml",
        global = true
    )]
    config: String,

    #[command(subcommand)]
    command: Option<Command>,
}

#[derive(Debug, Subcommand)]
enum Command {
    Run,
    Serve,
    SyncNow,
    InstallService,
    UninstallService,
    EnableAutostart,
    DisableAutostart,
    AutostartStatus,
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    init_logging();

    let cli = Cli::parse();
    match cli.command.unwrap_or(Command::Run) {
        Command::Run => run_daemon(&cli.config).await,
        Command::Serve => serve(&cli.config).await,
        Command::SyncNow => sync_now(&cli.config).await,
        Command::InstallService => windows::service::install_service(),
        Command::UninstallService => windows::service::uninstall_service(),
        Command::EnableAutostart => windows::autostart::enable_autostart(),
        Command::DisableAutostart => windows::autostart::disable_autostart(),
        Command::AutostartStatus => {
            let enabled = windows::autostart::is_autostart_enabled()?;
            info!(stage = "windows.autostart", enabled, "开机自启动状态已读取");
            Ok(())
        }
    }
}

async fn serve(config_path: &str) -> anyhow::Result<()> {
    let cfg = AppConfig::load(config_path)
        .with_context(|| format!("failed to load config from {config_path}"))?;
    info!(
        stage = "server.startup",
        bind = %cfg.server.bind,
        route = %cfg.server.route,
        persist = cfg.server.persist,
        "准备启动称重数据接收服务"
    );
    let db_pool = open_persist_pool(&cfg).await;
    server::run_http_server(cfg.server, db_pool).await
}

/// Open a SQLite pool for inbound persistence when `server.persist` is enabled.
///
/// Persistence is best-effort: if the connection cannot be established, the
/// receive server still runs and logs payloads (returns `None`).
async fn open_persist_pool(cfg: &AppConfig) -> Option<db::DbPool> {
    if !cfg.server.persist {
        return None;
    }
    match db::connect(&cfg.database).await {
        Ok(pool) => {
            info!(
                stage = "database.ready",
                database_url = %cfg.database.url,
                "接收服务已连接本地 SQLite 缓存用于持久化"
            );
            Some(pool)
        }
        Err(error) => {
            warn!(
                stage = "database.error",
                %error,
                "接收服务连接 SQLite 失败，本次仅记录日志不落库"
            );
            None
        }
    }
}

fn init_logging() {
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt()
        .json()
        .with_env_filter(filter)
        .with_current_span(false)
        .with_span_list(false)
        .init();
}

async fn sync_now(config_path: &str) -> anyhow::Result<()> {
    let (cfg, engine) = build_engine(config_path).await?;
    info!(
        stage = "sync.manual",
        endpoint = %cfg.api.endpoint,
        source = %cfg.sync.source,
        "开始执行手动同步"
    );
    let result = engine.sync_once().await?;
    info!(stage = "sync.manual", ?result, "手动同步完成");
    Ok(())
}

async fn run_daemon(config_path: &str) -> anyhow::Result<()> {
    info!(
        stage = "startup.begin",
        version = env!("CARGO_PKG_VERSION"),
        "称重数据同步系统正在启动"
    );

    let (cfg, engine) = build_engine(config_path).await?;

    if cfg.server.enabled {
        let server_cfg = cfg.server.clone();
        let db_pool = open_persist_pool(&cfg).await;
        tokio::spawn(async move {
            if let Err(error) = server::run_http_server(server_cfg, db_pool).await {
                error!(stage = "server.error", %error, "称重数据接收服务异常退出");
            }
        });
    }

    if cfg.sync.sync_on_start {
        match engine.sync_once().await {
            Ok(result) => info!(stage = "sync.startup", ?result, "启动同步完成"),
            Err(error) => warn!(stage = "sync.startup", %error, "启动同步失败，等待调度器重试"),
        }
    }

    #[cfg(feature = "cron")]
    {
        start_scheduler(cfg.clone(), engine).await?;
    }

    #[cfg(not(feature = "cron"))]
    {
        warn!(
            stage = "scheduler.disabled",
            "cron 特性未启用，定时同步不会启动"
        );
    }

    info!(stage = "startup.ready", "服务已就绪，等待同步信号");
    tokio::signal::ctrl_c()
        .await
        .context("failed to wait for Ctrl+C")?;
    info!(stage = "shutdown", "收到停止信号，服务正在关闭");
    Ok(())
}

#[derive(Debug, Clone)]
enum AppEngine {
    Local(SyncEngine),
    SqlServer(SqlServerSyncEngine),
}

impl AppEngine {
    async fn sync_once(&self) -> anyhow::Result<SyncOutcome> {
        match self {
            Self::Local(engine) => engine.sync_once().await,
            Self::SqlServer(engine) => engine.sync_once().await,
        }
    }
}

async fn build_engine(config_path: &str) -> anyhow::Result<(AppConfig, AppEngine)> {
    let cfg = AppConfig::load(config_path)
        .with_context(|| format!("failed to load config from {config_path}"))?;
    info!(
        stage = "config.loaded",
        path = config_path,
        source = %cfg.sync.source,
        "配置文件加载成功"
    );

    let engine = if cfg.sync.source.eq_ignore_ascii_case("sqlserver") {
        let source = SqlServerSource::new(cfg.sqlserver.clone())?;
        info!(
            stage = "sqlserver.ready",
            host = %cfg.sqlserver.host,
            port = cfg.sqlserver.port,
            database = %cfg.sqlserver.database,
            table = %cfg.sqlserver.table,
            mark_uploaded = cfg.sqlserver.mark_uploaded,
            "SQL Server 数据源已初始化"
        );
        AppEngine::SqlServer(SqlServerSyncEngine::new(
            cfg.api.clone(),
            cfg.sync.clone(),
            source,
        )?)
    } else {
        let pool = db::connect(&cfg.database).await?;
        info!(
            stage = "database.ready",
            database_url = %cfg.database.url,
            max_connections = cfg.database.max_connections,
            "本地 SQLite 缓存数据库已就绪"
        );
        AppEngine::Local(SyncEngine::new(pool, cfg.api.clone(), cfg.sync.clone())?)
    };

    info!(
        stage = "client.ready",
        endpoint = %cfg.api.endpoint,
        batch_size = cfg.sync.batch_size,
        "云端同步客户端已初始化"
    );
    Ok((cfg, engine))
}

#[cfg(feature = "cron")]
async fn start_scheduler(cfg: AppConfig, engine: AppEngine) -> anyhow::Result<()> {
    use tokio_cron_scheduler::{Job, JobScheduler};

    let scheduler = JobScheduler::new().await?;
    let guarded_engine = Arc::new(Mutex::new(engine));
    let job_engine = Arc::clone(&guarded_engine);
    let cron = cfg.sync.cron.clone();

    let job = Job::new_async(cron.as_str(), move |_job_id, _scheduler| {
        let job_engine = Arc::clone(&job_engine);
        Box::pin(async move {
            let engine = job_engine.lock().await;
            match engine.sync_once().await {
                Ok(result) => info!(stage = "sync.scheduled", ?result, "定时同步完成"),
                Err(error) => error!(stage = "sync.scheduled", %error, "定时同步失败"),
            }
        })
    })?;

    scheduler.add(job).await?;
    scheduler.start().await?;
    info!(stage = "scheduler.ready", cron = %cfg.sync.cron, "同步调度器已启动");
    Ok(())
}
