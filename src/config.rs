use std::{path::Path, time::Duration};

use anyhow::Context;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize)]
pub struct AppConfig {
    pub api: ApiConfig,
    pub database: DatabaseConfig,
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub sqlserver: SqlServerConfig,
    pub sync: SyncConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub endpoint: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct DatabaseConfig {
    #[serde(default = "default_database_url")]
    pub url: String,
    #[serde(default = "default_max_connections")]
    pub max_connections: u32,
}

#[derive(Debug, Clone, Deserialize)]
pub struct ServerConfig {
    #[serde(default)]
    pub enabled: bool,
    #[serde(default = "default_server_bind")]
    pub bind: String,
    #[serde(default = "default_server_route")]
    pub route: String,
    #[serde(default = "default_server_max_body_bytes")]
    pub max_body_bytes: usize,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SyncConfig {
    #[serde(default = "default_sync_source")]
    pub source: String,
    #[serde(default = "default_batch_size")]
    pub batch_size: u32,
    #[serde(default = "default_cron")]
    pub cron: String,
    #[serde(default = "default_true")]
    pub sync_on_start: bool,
    #[serde(default = "default_retry_initial_delay_ms")]
    pub retry_initial_delay_ms: u64,
    #[serde(default = "default_retry_max_delay_ms")]
    pub retry_max_delay_ms: u64,
    #[serde(default = "default_retry_max_elapsed_seconds")]
    pub retry_max_elapsed_seconds: u64,
}

#[derive(Debug, Clone, Deserialize)]
pub struct SqlServerConfig {
    #[serde(default = "default_sqlserver_host")]
    pub host: String,
    #[serde(default = "default_sqlserver_port")]
    pub port: u16,
    #[serde(default = "default_sqlserver_database")]
    pub database: String,
    #[serde(default = "default_sqlserver_username")]
    pub username: String,
    #[serde(default = "default_sqlserver_password")]
    pub password: String,
    #[serde(default = "default_sqlserver_table")]
    pub table: String,
    #[serde(default = "default_true")]
    pub trust_cert: bool,
    #[serde(default = "default_sqlserver_encryption")]
    pub encryption: String,
    #[serde(default = "default_true")]
    pub mark_uploaded: bool,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            bind: default_server_bind(),
            route: default_server_route(),
            max_body_bytes: default_server_max_body_bytes(),
        }
    }
}

impl Default for SqlServerConfig {
    fn default() -> Self {
        Self {
            host: default_sqlserver_host(),
            port: default_sqlserver_port(),
            database: default_sqlserver_database(),
            username: default_sqlserver_username(),
            password: default_sqlserver_password(),
            table: default_sqlserver_table(),
            trust_cert: default_true(),
            encryption: default_sqlserver_encryption(),
            mark_uploaded: default_true(),
        }
    }
}

impl AppConfig {
    pub fn load(path: impl AsRef<Path>) -> anyhow::Result<Self> {
        let path = path.as_ref();
        let builder = config::Config::builder()
            .add_source(config::File::from(path).required(false))
            .add_source(
                config::Environment::with_prefix("WDS")
                    .separator("__")
                    .try_parsing(true),
            );

        builder
            .build()
            .context("failed to build configuration")?
            .try_deserialize()
            .context("failed to deserialize configuration")
    }
}

impl ApiConfig {
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds)
    }
}

impl SyncConfig {
    pub fn retry_initial_delay(&self) -> Duration {
        Duration::from_millis(self.retry_initial_delay_ms)
    }

    pub fn retry_max_delay(&self) -> Duration {
        Duration::from_millis(self.retry_max_delay_ms)
    }

    pub fn retry_max_elapsed(&self) -> Duration {
        Duration::from_secs(self.retry_max_elapsed_seconds)
    }
}

impl SqlServerConfig {
    pub fn is_plaintext(&self) -> bool {
        self.encryption.eq_ignore_ascii_case("not_supported")
            || self.encryption.eq_ignore_ascii_case("none")
            || self.encryption.eq_ignore_ascii_case("plaintext")
    }

    pub fn is_required_encryption(&self) -> bool {
        self.encryption.eq_ignore_ascii_case("required")
            || self.encryption.eq_ignore_ascii_case("true")
            || self.encryption.eq_ignore_ascii_case("on")
    }
}

fn default_database_url() -> String {
    "sqlite://data/weighing_sync.db".to_owned()
}

fn default_server_bind() -> String {
    "0.0.0.0:8080".to_owned()
}

fn default_server_route() -> String {
    "/weighing-data-sync/put".to_owned()
}

fn default_server_max_body_bytes() -> usize {
    16 * 1024 * 1024
}

fn default_sync_source() -> String {
    "sqlserver".to_owned()
}

fn default_batch_size() -> u32 {
    100
}

fn default_cron() -> String {
    "0 */5 * * * *".to_owned()
}

fn default_max_connections() -> u32 {
    5
}

fn default_retry_initial_delay_ms() -> u64 {
    1_000
}

fn default_retry_max_delay_ms() -> u64 {
    60_000
}

fn default_retry_max_elapsed_seconds() -> u64 {
    300
}

fn default_timeout_seconds() -> u64 {
    30
}

fn default_true() -> bool {
    true
}

fn default_sqlserver_host() -> String {
    "192.168.10.251".to_owned()
}

fn default_sqlserver_port() -> u16 {
    1433
}

fn default_sqlserver_database() -> String {
    "yunfu".to_owned()
}

fn default_sqlserver_username() -> String {
    "sa".to_owned()
}

fn default_sqlserver_password() -> String {
    "yusilong".to_owned()
}

fn default_sqlserver_table() -> String {
    "tbl_weightInfo".to_owned()
}

fn default_sqlserver_encryption() -> String {
    "off".to_owned()
}
