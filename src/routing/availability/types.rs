// Route availability results and quota evidence shared by policy and readers.
// Contains only observed state and marker/probe metadata.
use chrono::{DateTime, NaiveDateTime, Utc};
use std::time::{Duration, SystemTime};

#[derive(Clone, Debug, PartialEq)]
pub struct RouteAvailability {
    pub status: RouteStatus,
    pub wall: QuotaWall,
    pub ends: HoldEnd,
    pub why: String,
    pub marker: Option<MarkerEvidence>,
    pub probe: Option<ProbeEvidence>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RouteStatus {
    Dispatchable,
    Degraded,
    Held,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum QuotaWall {
    Clock,
    Windowed,
    Prepaid,
    PlanChange,
    Transient,
    None,
}

#[derive(Clone, Debug, PartialEq)]
pub enum HoldEnd {
    At(NaiveDateTime),
    ClearLimit { slug: String },
    SnapshotDatedWindow,
    Cooldown,
    Nothing,
}

#[derive(Clone, Debug, PartialEq)]
pub struct MarkerEvidence {
    pub mtime: SystemTime,
}

#[derive(Clone, Debug, PartialEq)]
pub struct ProbeEvidence {
    pub provider: String,
    pub fetched_at: DateTime<Utc>,
    pub age: Duration,
    pub stale: bool,
    pub ok: bool,
    pub windows: Vec<WindowView>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct WindowView {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<DateTime<Utc>>,
    pub group: Option<String>,
}
