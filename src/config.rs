use std::{path::Path, time::Duration};

use anyhow::{Context, bail};
use serde::Deserialize;

/// Placeholder shipped as the default SQL Server host.
///
/// Real on-premise hosts must be supplied via the config file or the
/// `WDS__SQLSERVER__HOST` environment variable; [`SqlServerConfig::validate`]
/// rejects this value.
const PLACEHOLDER_HOST: &str = "your-sqlserver-host";
/// Placeholder shipped as the default SQL Server password.
const PLACEHOLDER_PASSWORD: &str = "CHANGE_ME_VIA_ENV";

/// Top-level application configuration, loaded from a TOML file with `WDS__*`
/// environment-variable overrides. See [`AppConfig::load`].
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

/// Cloud upload endpoint, optional bearer token and request timeout.
#[derive(Debug, Clone, Deserialize)]
pub struct ApiConfig {
    pub endpoint: String,
    #[serde(default)]
    pub api_key: Option<String>,
    #[serde(default = "default_timeout_seconds")]
    pub timeout_seconds: u64,
}

/// Local SQLite cache connection settings.
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
    /// Optional bearer token required on the receive endpoint. When `None`, the
    /// endpoint accepts all requests (log-only / development mode).
    #[serde(default)]
    pub api_key: Option<String>,
    /// When `true`, persist every received batch as raw JSON into the local
    /// `inbound_payloads` SQLite table (auditing / local-first receipt).
    #[serde(default)]
    pub persist: bool,
}

/// Sync tuning: source selection, batch size, cron schedule and retry policy.
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

/// SQL Server source connection.
///
/// Sensitive fields (`host`/`username`/`password`) ship as placeholders and
/// must be supplied via the config file or `WDS__SQLSERVER__*` environment
/// variables; [`SqlServerConfig::validate`] rejects the placeholders.
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
            api_key: None,
            persist: false,
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
    /// Load configuration layered from a TOML file (optional) and `WDS__*`
    /// environment variables (`WDS__SECTION__KEY`, e.g. `WDS__API__ENDPOINT`).
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
    /// HTTP request timeout as a [`Duration`].
    pub fn timeout(&self) -> Duration {
        Duration::from_secs(self.timeout_seconds)
    }
}

impl SyncConfig {
    /// Initial (first) retry delay for the exponential backoff.
    pub fn retry_initial_delay(&self) -> Duration {
        Duration::from_millis(self.retry_initial_delay_ms)
    }

    /// Maximum delay between retries (backoff cap).
    pub fn retry_max_delay(&self) -> Duration {
        Duration::from_millis(self.retry_max_delay_ms)
    }

    /// Total elapsed time after which retries give up.
    pub fn retry_max_elapsed(&self) -> Duration {
        Duration::from_secs(self.retry_max_elapsed_seconds)
    }
}

impl SqlServerConfig {
    /// Validate that real SQL Server credentials are configured.
    ///
    /// The shipped defaults intentionally do not contain a real host or password.
    /// Supply them via the `[sqlserver]` section of `config/default.toml` or the
    /// `WDS__SQLSERVER__HOST` / `WDS__SQLSERVER__USERNAME` / `WDS__SQLSERVER__PASSWORD`
    /// environment variables before enabling the `sqlserver` sync source.
    pub fn validate(&self) -> anyhow::Result<()> {
        if self.host.trim().is_empty() || self.host == PLACEHOLDER_HOST {
            bail!(
                "SQL Server host is not configured; set [sqlserver].host or WDS__SQLSERVER__HOST"
            );
        }
        if self.username.trim().is_empty() {
            bail!(
                "SQL Server username is not configured; set [sqlserver].username or WDS__SQLSERVER__USERNAME"
            );
        }
        if self.password.is_empty() || self.password == PLACEHOLDER_PASSWORD {
            bail!(
                "SQL Server password is not configured; set [sqlserver].password or WDS__SQLSERVER__PASSWORD"
            );
        }
        Ok(())
    }

    /// `true` when TDS encryption should be disabled (`off`/`none`/`plaintext`).
    pub fn is_plaintext(&self) -> bool {
        self.encryption.eq_ignore_ascii_case("not_supported")
            || self.encryption.eq_ignore_ascii_case("none")
            || self.encryption.eq_ignore_ascii_case("plaintext")
    }

    /// `true` when TDS encryption is mandatory (`required`/`true`/`on`).
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
    PLACEHOLDER_HOST.to_owned()
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
    String::new()
}

fn default_sqlserver_table() -> String {
    "tbl_weightInfo".to_owned()
}

fn default_sqlserver_encryption() -> String {
    "off".to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_use_placeholders_not_real_credentials() {
        assert_eq!(default_sqlserver_host(), PLACEHOLDER_HOST);
        assert!(default_sqlserver_password().is_empty());
        assert_eq!(default_sqlserver_database(), "yunfu");
        assert_eq!(default_sqlserver_username(), "sa");
    }

    #[test]
    fn validate_requires_real_credentials() {
        let mut cfg = SqlServerConfig::default();
        // Shipped defaults are placeholders -> rejected.
        assert!(cfg.validate().is_err());

        cfg.host = "db.local".to_owned();
        cfg.username = "sa".to_owned();
        cfg.password = PLACEHOLDER_PASSWORD.to_owned();
        assert!(cfg.validate().is_err(), "placeholder password rejected");

        cfg.password = "real-secret".to_owned();
        assert!(cfg.validate().is_ok());
    }

    #[test]
    fn encryption_level_mapping() {
        let mut cfg = SqlServerConfig {
            encryption: "off".to_owned(),
            ..Default::default()
        };

        assert!(!cfg.is_plaintext());
        assert!(!cfg.is_required_encryption());

        cfg.encryption = "none".to_owned();
        assert!(cfg.is_plaintext());
        assert!(!cfg.is_required_encryption());

        cfg.encryption = "plaintext".to_owned();
        assert!(cfg.is_plaintext());

        cfg.encryption = "required".to_owned();
        assert!(cfg.is_required_encryption());

        cfg.encryption = "on".to_owned();
        assert!(cfg.is_required_encryption());

        cfg.encryption = "true".to_owned();
        assert!(cfg.is_required_encryption());
    }
}
