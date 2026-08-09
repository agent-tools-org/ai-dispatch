// Derived task judgment from lifecycle and verification facts.
// Exports: TaskOutcome, UnverifiedReason, verify_required.
// Deps: TaskStatus and VerifyStatus.

use super::{TaskStatus, VerifyStatus};
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
                | VerifyStatus::TimedOut,
                false | true,
            ) => Self::InProgress,
            (
                TaskStatus::Failed,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut,
                false | true,
            ) => Self::Failed,
            (
                TaskStatus::Stopped,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut,
                false | true,
            ) => Self::Stopped,
            (
                TaskStatus::Skipped,
                VerifyStatus::Pending
                | VerifyStatus::Passed
                | VerifyStatus::Failed
                | VerifyStatus::Skipped
                | VerifyStatus::TimedOut,
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
