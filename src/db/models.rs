use anyhow::{Context, bail};
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use uom::si::{
    f64::Mass,
    mass::{kilogram, pound},
};
use uuid::Uuid;

/// A single weighing record, the domain model backed by the local SQLite cache.
///
/// Weight is stored in kilograms and round-tripped to pounds via [`uom`], so the
/// original unit is preserved for reporting while conversions stay type-safe.
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

/// Mass unit of the original reading.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WeightUnit {
    Kilogram,
    Pound,
}

/// Sync lifecycle state of a cached record.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SyncStatus {
    Pending,
    Synced,
    Failed,
}

impl WeighingRecord {
    /// Create a record from a kilogram reading.
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

    /// Create a record from a pound reading (converted to kilograms internally).
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

    /// The stored weight as a typed kilogram [`Mass`].
    pub fn mass_kg(&self) -> Mass {
        Mass::new::<kilogram>(self.weight_kg)
    }

    /// The stored weight converted to pounds.
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn weight_unit_parse_round_trip() {
        assert_eq!(WeightUnit::parse("kg").unwrap(), WeightUnit::Kilogram);
        assert_eq!(WeightUnit::parse("kilogram").unwrap(), WeightUnit::Kilogram);
        assert_eq!(WeightUnit::parse("lb").unwrap(), WeightUnit::Pound);
        assert_eq!(WeightUnit::parse("lbs").unwrap(), WeightUnit::Pound);
        assert!(WeightUnit::parse("ounce").is_err());
        assert_eq!(WeightUnit::Kilogram.as_str(), "kilogram");
        assert_eq!(WeightUnit::Pound.as_str(), "pound");
    }

    #[test]
    fn sync_status_parse_round_trip() {
        assert_eq!(SyncStatus::parse("pending").unwrap(), SyncStatus::Pending);
        assert_eq!(SyncStatus::parse("synced").unwrap(), SyncStatus::Synced);
        assert_eq!(SyncStatus::parse("failed").unwrap(), SyncStatus::Failed);
        assert!(SyncStatus::parse("done").is_err());
    }

    #[test]
    fn kg_to_lb_and_back_is_consistent() {
        let record = WeighingRecord::new_kg("T1", "S1", None, 1000.0, Utc::now()).unwrap();
        assert_eq!(record.weight_kg, 1000.0);
        // 1 kg == ~2.20462 lb; uom's factor chain introduces tiny f64 drift, so
        // assert within 0.1 lb rather than against the exact constant.
        assert!((record.weight_lb() - 2_204.622_6).abs() < 0.1);

        let from_lb = WeighingRecord::new_lb("T1", "S1", None, 2_204.622_6, Utc::now()).unwrap();
        // Round-trip kg -> lb -> kg should be very close.
        assert!((from_lb.weight_kg - 1000.0).abs() < 1e-3);
        assert_eq!(from_lb.original_unit, WeightUnit::Pound);
    }

    #[test]
    fn rejects_invalid_weight() {
        assert!(WeighingRecord::new_kg("T", "S", None, -1.0, Utc::now()).is_err());
        assert!(WeighingRecord::new_kg("T", "S", None, f64::NAN, Utc::now()).is_err());
        assert!(WeighingRecord::new_kg("T", "S", None, f64::INFINITY, Utc::now()).is_err());
        // zero is valid.
        assert!(WeighingRecord::new_kg("T", "S", None, 0.0, Utc::now()).is_ok());
    }

    #[test]
    fn rfc3339_round_trip_through_cache_model() {
        let now = Utc::now();
        let record = WeighingRecord::new_kg("T", "S", None, 10.0, now).unwrap();
        let model = crate::entity::weighing_record::Model {
            id: record.id.to_string(),
            ticket_no: record.ticket_no.clone(),
            scale_no: record.scale_no.clone(),
            plate_no: record.plate_no.clone(),
            weight_kg: record.weight_kg,
            original_unit: record.original_unit.as_str().to_owned(),
            measured_at: record.measured_at.to_rfc3339(),
            status: record.status.as_str().to_owned(),
            retry_count: record.retry_count,
            last_error: record.last_error.clone(),
            created_at: record.created_at.to_rfc3339(),
            updated_at: record.updated_at.to_rfc3339(),
        };
        let restored = WeighingRecord::try_from(model).unwrap();
        assert_eq!(restored.id, record.id);
        assert_eq!(restored.status, SyncStatus::Pending);
        assert!((restored.measured_at - now).num_milliseconds().abs() < 1);
    }
}
