// Knowledge graph store types and date parsing helpers.
// Exports: KgTriple, KgStats, parse_kg_timestamp.
// Deps: anyhow, chrono.

use anyhow::{Result, anyhow};
use chrono::{DateTime, Local, NaiveDate, Utc};

#[derive(Debug, Clone)]
pub struct KgTriple {
    pub id: i64,
    pub subject: String,
    pub predicate: String,
    pub object: String,
    pub valid_from: Option<DateTime<Local>>,
    pub valid_to: Option<DateTime<Local>>,
    pub source: Option<String>,
    pub created_at: DateTime<Local>,
}

#[derive(Debug, Clone, Copy)]
pub struct KgStats {
    pub triple_count: i64,
    pub active_triple_count: i64,
    pub entity_count: i64,
    pub predicate_count: i64,
}

pub(crate) fn parse_kg_timestamp(value: &str) -> Result<String> {
    if let Ok(parsed) = DateTime::parse_from_rfc3339(value) {
        return Ok(parsed.with_timezone(&Utc).to_rfc3339());
    }
    let date = NaiveDate::parse_from_str(value, "%Y-%m-%d")
        .map_err(|_| anyhow!("Invalid timestamp '{value}'. Use YYYY-MM-DD or RFC3339."))?;
    let datetime = date
        .and_hms_opt(0, 0, 0)
        .ok_or_else(|| anyhow!("Invalid date '{value}'."))?;
    Ok(datetime.and_utc().to_rfc3339())
}
