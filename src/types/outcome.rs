// Derived task judgment from lifecycle and verification facts.
// Exports: TaskOutcome, UnverifiedReason, verify_required.
// Deps: TaskStatus, VerifyStatus, and DeliveryAssessment.

use super::{DeliveryAssessment, TaskStatus, VerifyStatus};
use serde::Serialize;

#[cfg(test)]
#[path = "outcome_tests.rs"]
mod tests;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum TaskOutcome {
    Verified,
    Delivered,
    Unverified(UnverifiedReason),
    Broken,
    Failed,
    Stopped,
    Skipped,
    InProgress,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
pub enum UnverifiedReason {
    TimedOut,
    Infrastructure,
    NoResult,
}

impl TaskOutcome {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Verified => "verified",
            Self::Delivered => "delivered",
            Self::Unverified(_) => "unverified",
            Self::Broken => "broken",
            Self::Failed => "failed",
            Self::Stopped => "stopped",
            Self::Skipped => "skipped",
            Self::InProgress => "in_progress",
        }
    }

    pub fn verification_tag(self) -> Option<&'static str> {
        match self {
            Self::Broken => Some("VFAIL"),
            Self::Unverified(UnverifiedReason::TimedOut) => Some("VTIMEOUT"),
            Self::Unverified(UnverifiedReason::Infrastructure) => Some("VINFRA"),
            Self::Unverified(UnverifiedReason::NoResult) => Some("VNORESULT"),
            Self::Verified
            | Self::Delivered
            | Self::Failed
            | Self::Stopped
            | Self::Skipped
            | Self::InProgress => None,
        }
    }

    pub fn derive(
        status: TaskStatus,
        verify_status: VerifyStatus,
        verify_required: bool,
    ) -> Self {
        match (status, verify_status, verify_required) {
            (
                TaskStatus::Waiting
                | TaskStatus::Pending
                | TaskStatus::Running
                | TaskStatus::AwaitingInput
                | TaskStatus::Stalled,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut
                | VerifyStatus::InfrastructureFailure,
                false | true,
            ) => Self::InProgress,
            (
                TaskStatus::Failed,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut
                | VerifyStatus::InfrastructureFailure,
                false | true,
            ) => Self::Failed,
            (
                TaskStatus::Stopped,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut
                | VerifyStatus::InfrastructureFailure,
                false | true,
            ) => Self::Stopped,
            (
                TaskStatus::Skipped,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut
                | VerifyStatus::InfrastructureFailure,
                false | true,
            ) => Self::Skipped,
            (TaskStatus::Done | TaskStatus::Merged, VerifyStatus::Passed, false | true) => {
                Self::Verified
            }
            (TaskStatus::Done | TaskStatus::Merged, VerifyStatus::Failed, false | true) => {
                Self::Broken
            }
            (
                TaskStatus::Done | TaskStatus::Merged,
                VerifyStatus::TimedOut,
                false | true,
            ) => Self::Unverified(UnverifiedReason::TimedOut),
            (
                TaskStatus::Done | TaskStatus::Merged,
                VerifyStatus::InfrastructureFailure,
                false | true,
            ) => Self::Unverified(UnverifiedReason::Infrastructure),
            (
                TaskStatus::Done | TaskStatus::Merged,
                VerifyStatus::Skipped | VerifyStatus::Pending,
                true,
            ) => Self::Unverified(UnverifiedReason::NoResult),
            (
                TaskStatus::Done | TaskStatus::Merged,
                VerifyStatus::Skipped | VerifyStatus::Pending,
                false,
            ) => Self::Delivered,
        }
    }

    /// Fold a persisted delivery assessment into lifecycle×verify judgment.
    ///
    /// `HollowOutput` already means nothing was observed on any channel and
    /// the worktree had no changes — not merely empty stdout. `MissingFinalDelivery`
    /// means the agent finished without a final deliverable. Either case is a
    /// non-delivery and must not count as success for stats/advise, even when
    /// the process exited 0 and status is still `Done`.
    ///
    /// `EmptyDiff` alone is not demoted: a read-only audit can write a report
    /// (or a commit-only task can leave a clean tree) without code changes.
    pub fn with_delivery_assessment(self, delivery: Option<DeliveryAssessment>) -> Self {
        if !self.is_success() {
            return self;
        }
        match delivery {
            Some(DeliveryAssessment::HollowOutput)
            | Some(DeliveryAssessment::MissingFinalDelivery) => Self::Failed,
            Some(DeliveryAssessment::EmptyDiff) | None => self,
        }
    }

    pub fn is_success(self) -> bool {
        matches!(self, Self::Verified | Self::Delivered)
    }

    pub fn is_unverified(self) -> bool {
        matches!(self, Self::Unverified(_))
    }

    pub fn is_merge_candidate(self) -> bool {
        matches!(
            self,
            Self::Verified | Self::Delivered | Self::Unverified(_) | Self::Broken
        )
    }
}

pub fn verify_required(verify: Option<&str>) -> bool {
    let verify = verify.map(str::trim);
    !matches!(verify, None | Some("") | Some("none") | Some("false") | Some("skip"))
}
