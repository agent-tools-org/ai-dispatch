// Delivery assessment derived from persisted task facts.
// Exports: DeliveryAssessment plus mapping from VerifyStatus.
// Deps: serde and parent VerifyStatus enum.

use serde::Serialize;

use super::VerifyStatus;

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

    pub fn from_verify_status(verify_status: VerifyStatus) -> Option<Self> {
        match verify_status {
            VerifyStatus::EmptyDiff => Some(Self::EmptyDiff),
            VerifyStatus::HollowOutput => Some(Self::HollowOutput),
            _ => None,
        }
    }

    pub fn implies_no_changes(self) -> bool {
        matches!(self, Self::EmptyDiff | Self::HollowOutput)
    }
}
