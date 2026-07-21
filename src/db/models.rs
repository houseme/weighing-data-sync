use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uom::si::{
    f64::Mass,
    mass::{kilogram, pound},
};
use uuid::Uuid;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WeighingRecord {
    pub id: Uuid,
    pub ticket_no: String,
    pub scale_no: String,
    pub plate_no: Option<String>,
    pub weight_kg: f64,
    pub original_unit: WeightUnit,
    pub measured_at: DateTime<Utc>,
    pub status: SyncStatus,
    pub retry_count: i64,
    pub last_error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeightUnit {
    Kilogram,
    Pound,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Pending,
    Synced,
    Failed,
}

impl WeighingRecord {
    pub fn new_kg(
        ticket_no: impl Into<String>,
        scale_no: impl Into<String>,
        plate_no: Option<String>,
        weight_kg: f64,
        measured_at: DateTime<Utc>,
    ) -> anyhow::Result<Self> {
        Self::new(
            ticket_no,
            scale_no,
            plate_no,
            Mass::new::<kilogram>(weight_kg),
            WeightUnit::Kilogram,
            measured_at,
        )
    }

    pub fn new_lb(
        ticket_no: impl Into<String>,
        scale_no: impl Into<String>,
        plate_no: Option<String>,
        weight_lb: f64,
        measured_at: DateTime<Utc>,
    ) -> anyhow::Result<Self> {
        Self::new(
            ticket_no,
            scale_no,
            plate_no,
            Mass::new::<pound>(weight_lb),
            WeightUnit::Pound,
            measured_at,
        )
    }

    pub fn mass_kg(&self) -> Mass {
        Mass::new::<kilogram>(self.weight_kg)
    }

    pub fn weight_lb(&self) -> f64 {
        self.mass_kg().get::<pound>()
    }

    fn new(
        ticket_no: impl Into<String>,
        scale_no: impl Into<String>,
        plate_no: Option<String>,
        mass: Mass,
        original_unit: WeightUnit,
        measured_at: DateTime<Utc>,
    ) -> anyhow::Result<Self> {
        let weight_kg = mass.get::<kilogram>();
        if !weight_kg.is_finite() || weight_kg < 0.0 {
            bail!("weight must be a finite non-negative number");
        }

        let now = Utc::now();
        Ok(Self {
            id: Uuid::new_v4(),
            ticket_no: ticket_no.into(),
            scale_no: scale_no.into(),
            plate_no,
            weight_kg,
            original_unit,
            measured_at,
            status: SyncStatus::Pending,
            retry_count: 0,
            last_error: None,
            created_at: now,
            updated_at: now,
        })
    }
}

impl TryFrom<crate::entity::weighing_record::Model> for WeighingRecord {
    type Error = anyhow::Error;

    fn try_from(model: crate::entity::weighing_record::Model) -> anyhow::Result<Self> {
        Ok(Self {
            id: Uuid::parse_str(&model.id).with_context(|| format!("invalid uuid {}", model.id))?,
            ticket_no: model.ticket_no,
            scale_no: model.scale_no,
            plate_no: model.plate_no,
            weight_kg: model.weight_kg,
            original_unit: WeightUnit::parse(&model.original_unit)?,
            measured_at: parse_rfc3339(&model.measured_at)?,
            status: SyncStatus::parse(&model.status)?,
            retry_count: model.retry_count,
            last_error: model.last_error,
            created_at: parse_rfc3339(&model.created_at)?,
            updated_at: parse_rfc3339(&model.updated_at)?,
        })
    }
}

impl WeightUnit {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Kilogram => "kilogram",
            Self::Pound => "pound",
        }
    }

    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "kilogram" | "kg" => Ok(Self::Kilogram),
            "pound" | "lb" | "lbs" => Ok(Self::Pound),
            other => bail!("unsupported weight unit {other}"),
        }
    }
}

impl SyncStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Synced => "synced",
            Self::Failed => "failed",
        }
    }

    fn parse(value: &str) -> anyhow::Result<Self> {
        match value {
            "pending" => Ok(Self::Pending),
            "synced" => Ok(Self::Synced),
            "failed" => Ok(Self::Failed),
            other => bail!("unsupported sync status {other}"),
        }
    }
}

fn parse_rfc3339(value: &str) -> anyhow::Result<DateTime<Utc>> {
    Ok(DateTime::parse_from_rfc3339(value)
        .with_context(|| format!("invalid RFC3339 datetime {value}"))?
        .with_timezone(&Utc))
}
