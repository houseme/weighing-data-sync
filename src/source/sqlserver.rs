use anyhow::{Context, bail};
use chrono::NaiveDateTime;
use rust_decimal::Decimal;
use serde_json::{Map, Number, Value};
use tiberius::{AuthMethod, Client, ColumnData, ColumnType, Config, EncryptionLevel, Row};
use tokio::net::TcpStream;
use tokio_util::compat::{Compat, TokioAsyncWriteCompatExt};
use tracing::info;

use crate::config::SqlServerConfig;

type TdsClient = Client<Compat<TcpStream>>;

const WEIGHT_INFO_COLUMNS: &[&str] = &[
    "serialNo",
    "sysNo",
    "setId",
    "cardNo",
    "plateNo",
    "weightType",
    "transportUnit",
    "forwardingUnit",
    "consigneeUnit",
    "goodsName",
    "goodsSpec",
    "grossWeight",
    "tareWeight",
    "netWeight",
    "buckleWeight",
    "actualWeight",
    "weightUnit",
    "unitPrice",
    "sumAmt",
    "scaleNum",
    "squareNum",
    "weighingFee",
    "grossStation",
    "tareStation",
    "grossMan",
    "tareMan",
    "grossTime",
    "tareTime",
    "firstTime",
    "secondTime",
    "updateTime",
    "printNum",
    "isCancle",
    "isUploadLocal",
    "isUploadCloud",
    "strBackup1",
    "strBackup2",
    "strBackup3",
    "strBackup4",
    "strBackup5",
    "strBackup6",
    "strBackup7",
    "strBackup8",
    "strBackup9",
    "numBackup1",
    "numBackup2",
    "numBackup3",
    "numBackup4",
    "numBackup5",
    "numBackup6",
    "numBackup7",
    "numBackup8",
    "numBackup9",
    "timeBackup1",
    "timeBackup2",
    "timeBackup3",
    "fGuid",
    "fID",
    "relNo",
    "relSer",
    "regNo",
    "dataType",
    "dataLog",
    "isFinish",
    "remark",
    "del_flag",
];

#[derive(Debug, Clone)]
pub struct SqlServerWeightRecord {
    pub serial_no: String,
    pub data: Map<String, Value>,
}

#[derive(Debug, Clone)]
pub struct SqlServerSource {
    cfg: SqlServerConfig,
}

impl SqlServerSource {
    pub fn new(cfg: SqlServerConfig) -> anyhow::Result<Self> {
        validate_identifier(&cfg.table)?;
        Ok(Self { cfg })
    }

    pub fn database_name(&self) -> &str {
        &self.cfg.database
    }

    pub fn table_name(&self) -> &str {
        &self.cfg.table
    }

    pub fn mark_uploaded_enabled(&self) -> bool {
        self.cfg.mark_uploaded
    }

    pub async fn fetch_pending(&self, limit: u32) -> anyhow::Result<Vec<SqlServerWeightRecord>> {
        let mut client = self.connect().await?;
        let sql = format!(
            r#"
            SELECT TOP ({limit}) {columns}
            FROM {table}
            WHERE ISNULL([isUploadCloud], 0) = 0
              AND ISNULL([del_flag], 0) = 0
            ORDER BY ISNULL([updateTime], ISNULL([secondTime], ISNULL([firstTime], [grossTime]))) ASC
            "#,
            columns = select_columns(),
            table = quoted_identifier(&self.cfg.table),
        );

        let rows = client
            .query(sql, &[])
            .await
            .context("failed to query tbl_weightInfo")?
            .into_first_result()
            .await
            .context("failed to read tbl_weightInfo rows")?;

        rows.into_iter().map(row_to_record).collect()
    }

    pub async fn mark_uploaded(&self, serial_nos: &[String]) -> anyhow::Result<()> {
        if serial_nos.is_empty() || !self.cfg.mark_uploaded {
            return Ok(());
        }

        let mut client = self.connect().await?;
        let sql = format!(
            "UPDATE {table} SET [isUploadCloud] = 1 WHERE [serialNo] = @P1",
            table = quoted_identifier(&self.cfg.table)
        );

        for serial_no in serial_nos {
            client
                .execute(&sql, &[serial_no])
                .await
                .with_context(|| format!("failed to mark serialNo {serial_no} as uploaded"))?;
        }

        info!(
            stage = "sqlserver.mark_uploaded",
            count = serial_nos.len(),
            "SQL Server 云上传状态已更新"
        );
        Ok(())
    }

    async fn connect(&self) -> anyhow::Result<TdsClient> {
        let mut config = Config::new();
        config.host(&self.cfg.host);
        config.port(self.cfg.port);
        config.database(&self.cfg.database);
        config.application_name("weighing-data-sync");
        config.authentication(AuthMethod::sql_server(
            &self.cfg.username,
            &self.cfg.password,
        ));

        if self.cfg.trust_cert {
            config.trust_cert();
        }

        let encryption = if self.cfg.is_plaintext() {
            EncryptionLevel::NotSupported
        } else if self.cfg.is_required_encryption() {
            EncryptionLevel::Required
        } else {
            EncryptionLevel::Off
        };
        config.encryption(encryption);

        let tcp = TcpStream::connect(config.get_addr())
            .await
            .with_context(|| format!("failed to connect SQL Server {}", config.get_addr()))?;
        tcp.set_nodelay(true)
            .context("failed to enable TCP_NODELAY for SQL Server connection")?;

        Client::connect(config, tcp.compat_write())
            .await
            .context("failed to login SQL Server")
    }
}

fn row_to_record(row: Row) -> anyhow::Result<SqlServerWeightRecord> {
    let serial_no = get_string(&row, "serialNo")?
        .filter(|value| !value.trim().is_empty())
        .context("tbl_weightInfo row missing serialNo")?;

    let mut data = Map::new();
    for (column, value) in row.cells() {
        data.insert(
            column.name().to_owned(),
            column_value_to_json(&row, column.name(), column.column_type(), value)?,
        );
    }

    Ok(SqlServerWeightRecord { serial_no, data })
}

fn column_value_to_json(
    row: &Row,
    column_name: &str,
    column_type: ColumnType,
    value: &ColumnData<'static>,
) -> anyhow::Result<Value> {
    if is_null(value) {
        return Ok(Value::Null);
    }

    let json = match value {
        ColumnData::U8(Some(value)) => Value::Number(Number::from(*value)),
        ColumnData::I16(Some(value)) => Value::Number(Number::from(*value)),
        ColumnData::I32(Some(value)) => Value::Number(Number::from(*value)),
        ColumnData::I64(Some(value)) => Value::Number(Number::from(*value)),
        ColumnData::F32(Some(value)) => json_number(f64::from(*value))?,
        ColumnData::F64(Some(value)) => json_number(*value)?,
        ColumnData::Bit(Some(value)) => Value::Bool(*value),
        ColumnData::String(Some(value)) => Value::String(value.to_string()),
        ColumnData::Guid(Some(value)) => Value::String(value.to_string()),
        ColumnData::Numeric(Some(value)) => Value::String(format_numeric(*value)),
        ColumnData::Xml(Some(value)) => Value::String(value.to_string()),
        ColumnData::Binary(Some(value)) => Value::String(format!("<{} bytes>", value.len())),
        ColumnData::DateTime(Some(_)) | ColumnData::SmallDateTime(Some(_)) => {
            Value::String(format_datetime(row, column_name)?)
        }
        ColumnData::DateTime2(Some(_)) | ColumnData::DateTimeOffset(Some(_)) => {
            Value::String(format_datetime(row, column_name)?)
        }
        ColumnData::Date(Some(_)) | ColumnData::Time(Some(_)) => {
            Value::String(format_datetime(row, column_name)?)
        }
        _ => match column_type {
            ColumnType::Decimaln
            | ColumnType::Numericn
            | ColumnType::Money
            | ColumnType::Money4 => get_decimal(row, column_name)?
                .map_or(Value::Null, |value| Value::String(value.to_string())),
            _ => Value::String(format!("{value:?}")),
        },
    };

    Ok(json)
}

fn get_string(row: &Row, column: &str) -> anyhow::Result<Option<String>> {
    Ok(row.try_get::<&str, _>(column)?.map(ToOwned::to_owned))
}

fn get_decimal(row: &Row, column: &str) -> anyhow::Result<Option<Decimal>> {
    Ok(row.try_get::<Decimal, _>(column)?)
}

fn format_datetime(row: &Row, column: &str) -> anyhow::Result<String> {
    let value = row
        .try_get::<NaiveDateTime, _>(column)?
        .with_context(|| format!("datetime column {column} is null"))?;
    Ok(value.format("%Y-%m-%d %H:%M:%S").to_string())
}

fn json_number(value: f64) -> anyhow::Result<Value> {
    Number::from_f64(value)
        .map(Value::Number)
        .with_context(|| format!("invalid floating point value {value}"))
}

fn is_null(value: &ColumnData<'static>) -> bool {
    matches!(
        value,
        ColumnData::U8(None)
            | ColumnData::I16(None)
            | ColumnData::I32(None)
            | ColumnData::I64(None)
            | ColumnData::F32(None)
            | ColumnData::F64(None)
            | ColumnData::Bit(None)
            | ColumnData::String(None)
            | ColumnData::Guid(None)
            | ColumnData::Binary(None)
            | ColumnData::Numeric(None)
            | ColumnData::Xml(None)
            | ColumnData::DateTime(None)
            | ColumnData::SmallDateTime(None)
            | ColumnData::Time(None)
            | ColumnData::Date(None)
            | ColumnData::DateTime2(None)
            | ColumnData::DateTimeOffset(None)
    )
}

fn format_numeric(value: tiberius::numeric::Numeric) -> String {
    let scale = value.scale() as u32;
    let raw = value.value();
    if scale == 0 {
        return raw.to_string();
    }

    let negative = raw < 0;
    let abs = raw.abs();
    let factor = 10_i128.pow(scale);
    let integer = abs / factor;
    let fraction = abs % factor;
    let sign = if negative { "-" } else { "" };

    format!("{sign}{integer}.{fraction:0width$}", width = scale as usize)
}

fn select_columns() -> String {
    WEIGHT_INFO_COLUMNS
        .iter()
        .map(|column| quoted_identifier(column))
        .collect::<Vec<_>>()
        .join(", ")
}

fn quoted_identifier(identifier: &str) -> String {
    format!("[{}]", identifier.replace(']', "]]"))
}

fn validate_identifier(identifier: &str) -> anyhow::Result<()> {
    if identifier.is_empty()
        || !identifier
            .chars()
            .all(|ch| ch.is_ascii_alphanumeric() || ch == '_' || ch == '.')
    {
        bail!("invalid SQL Server identifier: {identifier}");
    }
    Ok(())
}
