// Delivery assessment derived from persisted task facts.
// Exports: DeliveryAssessment parse/display helpers for stored task metadata.
// Deps: serde only.

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum DeliveryAssessment {
    EmptyDiff,
    HollowOutput,
    MissingFinalDelivery,
}

impl DeliveryAssessment {

    pub fn implies_no_changes(self) -> bool {
        matches!(
            self,
            Self::EmptyDiff | Self::HollowOutput | Self::MissingFinalDelivery
        )
    }
}

super::string_values::string_values!(
    DeliveryAssessment, as_str(&self), pub parse_str {
        EmptyDiff => "empty_diff",
        HollowOutput => "hollow_output",
        MissingFinalDelivery => "missing_final_delivery",
    }
);
