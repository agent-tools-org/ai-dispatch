// Delivery assessment derived from persisted task facts.
// Exports: DeliveryAssessment parse/display helpers for stored task metadata.
// Deps: serde only.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeliveryAssessment {
    EmptyDiff,
    HollowOutput,
}

impl DeliveryAssessment {
    pub fn as_str(&self) -> &'static str {
        match self {
            Self::EmptyDiff => "empty_diff",
            Self::HollowOutput => "hollow_output",
        }
    }

    pub fn parse_str(value: &str) -> Option<Self> {
        match value {
            "empty_diff" => Some(Self::EmptyDiff),
            "hollow_output" => Some(Self::HollowOutput),
            _ => None,
        }
    }

    pub fn implies_no_changes(self) -> bool {
        matches!(self, Self::EmptyDiff | Self::HollowOutput)
    }
}
