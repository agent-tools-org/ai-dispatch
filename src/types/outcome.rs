// Derived task judgment from lifecycle and verification facts.
// Exports: TaskOutcome, UnverifiedReason, verify_required.
// Deps: TaskStatus and VerifyStatus.

use super::{TaskStatus, VerifyStatus};
use serde::Serialize;

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
}

pub fn verify_required(verify: Option<&str>) -> bool {
    !matches!(verify, None | Some("") | Some("none") | Some("false") | Some("skip"))
}

#[cfg(test)]
mod tests {
    use super::{verify_required, TaskOutcome, UnverifiedReason};
    use crate::types::{TaskStatus, VerifyStatus};

    const VERIFY_STATUSES: [VerifyStatus; 5] = [
        VerifyStatus::Pending,
        VerifyStatus::Passed,
        VerifyStatus::Failed,
        VerifyStatus::Skipped,
        VerifyStatus::TimedOut,
    ];

    #[test]
    fn golden_cartesian_derivation_table() {
        for status in TaskStatus::ALL {
            for verify_status in VERIFY_STATUSES {
                for verify_required in [false, true] {
                    let expected = match (status, verify_status, verify_required) {
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
                        ) => TaskOutcome::InProgress,
                        (
                            TaskStatus::Failed,
                            VerifyStatus::Pending
                            | VerifyStatus::Passed
                            | VerifyStatus::Failed
                            | VerifyStatus::Skipped
                            | VerifyStatus::TimedOut,
                            false | true,
                        ) => TaskOutcome::Failed,
                        (
                            TaskStatus::Stopped,
                            VerifyStatus::Pending
                            | VerifyStatus::Passed
                            | VerifyStatus::Failed
                            | VerifyStatus::Skipped
                            | VerifyStatus::TimedOut,
                            false | true,
                        ) => TaskOutcome::Stopped,
                        (
                            TaskStatus::Skipped,
                            VerifyStatus::Pending
                            | VerifyStatus::Passed
                            | VerifyStatus::Failed
                            | VerifyStatus::Skipped
                            | VerifyStatus::TimedOut,
                            false | true,
                        ) => TaskOutcome::Skipped,
                        (TaskStatus::Done | TaskStatus::Merged, VerifyStatus::Passed, false | true) => {
                            TaskOutcome::Verified
                        }
                        (TaskStatus::Done | TaskStatus::Merged, VerifyStatus::Failed, false | true) => {
                            TaskOutcome::Broken
                        }
                        (
                            TaskStatus::Done | TaskStatus::Merged,
                            VerifyStatus::TimedOut,
                            false | true,
                        ) => TaskOutcome::Unverified(UnverifiedReason::TimedOut),
                        (
                            TaskStatus::Done | TaskStatus::Merged,
                            VerifyStatus::Skipped | VerifyStatus::Pending,
                            true,
                        ) => TaskOutcome::Unverified(UnverifiedReason::NoResult),
                        (
                            TaskStatus::Done | TaskStatus::Merged,
                            VerifyStatus::Skipped | VerifyStatus::Pending,
                            false,
                        ) => TaskOutcome::Delivered,
                    };

                    assert_eq!(
                        TaskOutcome::derive(status, verify_status, verify_required),
                        expected,
                        "status={}, verify_status={}, verify_required={verify_required}",
                        status.as_str(),
                        verify_status.as_str(),
                    );
                }
            }
        }
    }

    #[test]
    fn verify_required_reads_the_verify_column_contract() {
        assert!(!verify_required(None));
        assert!(!verify_required(Some("")));
        assert!(!verify_required(Some("none")));
        assert!(!verify_required(Some("false")));
        assert!(!verify_required(Some("skip")));
        assert!(verify_required(Some("auto")));
        assert!(verify_required(Some("cargo test")));
    }
}
