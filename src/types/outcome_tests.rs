// Exhaustive contract tests for task outcome derivation and verification rules.
// Exports: none; loaded by outcome.rs under `#[cfg(test)]`.
// Deps: crate::types public outcome and status re-exports.

use crate::types::{verify_required, TaskOutcome, TaskStatus, UnverifiedReason, VerifyStatus};

const GOLDEN_TABLE: [(TaskStatus, VerifyStatus, bool, TaskOutcome); 100] = [
    (TaskStatus::Waiting, VerifyStatus::Pending, false, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Pending, true, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Passed, false, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Passed, true, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Failed, false, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Failed, true, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Skipped, false, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::Skipped, true, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::TimedOut, false, TaskOutcome::InProgress),
    (TaskStatus::Waiting, VerifyStatus::TimedOut, true, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Pending, false, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Pending, true, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Passed, false, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Passed, true, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Failed, false, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Failed, true, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Skipped, false, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::Skipped, true, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::TimedOut, false, TaskOutcome::InProgress),
    (TaskStatus::Pending, VerifyStatus::TimedOut, true, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Pending, false, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Pending, true, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Passed, false, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Passed, true, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Failed, false, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Failed, true, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Skipped, false, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::Skipped, true, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::TimedOut, false, TaskOutcome::InProgress),
    (TaskStatus::Running, VerifyStatus::TimedOut, true, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Pending, false, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Pending, true, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Passed, false, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Passed, true, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Failed, false, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Failed, true, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Skipped, false, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::Skipped, true, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::TimedOut, false, TaskOutcome::InProgress),
    (TaskStatus::AwaitingInput, VerifyStatus::TimedOut, true, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Pending, false, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Pending, true, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Passed, false, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Passed, true, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Failed, false, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Failed, true, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Skipped, false, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::Skipped, true, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::TimedOut, false, TaskOutcome::InProgress),
    (TaskStatus::Stalled, VerifyStatus::TimedOut, true, TaskOutcome::InProgress),
    (TaskStatus::Done, VerifyStatus::Pending, false, TaskOutcome::Delivered),
    (TaskStatus::Done, VerifyStatus::Pending, true, TaskOutcome::Unverified(UnverifiedReason::NoResult)),
    (TaskStatus::Done, VerifyStatus::Passed, false, TaskOutcome::Verified),
    (TaskStatus::Done, VerifyStatus::Passed, true, TaskOutcome::Verified),
    (TaskStatus::Done, VerifyStatus::Failed, false, TaskOutcome::Broken),
    (TaskStatus::Done, VerifyStatus::Failed, true, TaskOutcome::Broken),
    (TaskStatus::Done, VerifyStatus::Skipped, false, TaskOutcome::Delivered),
    (TaskStatus::Done, VerifyStatus::Skipped, true, TaskOutcome::Unverified(UnverifiedReason::NoResult)),
    (TaskStatus::Done, VerifyStatus::TimedOut, false, TaskOutcome::Unverified(UnverifiedReason::TimedOut)),
    (TaskStatus::Done, VerifyStatus::TimedOut, true, TaskOutcome::Unverified(UnverifiedReason::TimedOut)),
    (TaskStatus::Merged, VerifyStatus::Pending, false, TaskOutcome::Delivered),
    (TaskStatus::Merged, VerifyStatus::Pending, true, TaskOutcome::Unverified(UnverifiedReason::NoResult)),
    (TaskStatus::Merged, VerifyStatus::Passed, false, TaskOutcome::Verified),
    (TaskStatus::Merged, VerifyStatus::Passed, true, TaskOutcome::Verified),
    (TaskStatus::Merged, VerifyStatus::Failed, false, TaskOutcome::Broken),
    (TaskStatus::Merged, VerifyStatus::Failed, true, TaskOutcome::Broken),
    (TaskStatus::Merged, VerifyStatus::Skipped, false, TaskOutcome::Delivered),
    (TaskStatus::Merged, VerifyStatus::Skipped, true, TaskOutcome::Unverified(UnverifiedReason::NoResult)),
    (TaskStatus::Merged, VerifyStatus::TimedOut, false, TaskOutcome::Unverified(UnverifiedReason::TimedOut)),
    (TaskStatus::Merged, VerifyStatus::TimedOut, true, TaskOutcome::Unverified(UnverifiedReason::TimedOut)),
    (TaskStatus::Failed, VerifyStatus::Pending, false, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Pending, true, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Passed, false, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Passed, true, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Failed, false, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Failed, true, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Skipped, false, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::Skipped, true, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::TimedOut, false, TaskOutcome::Failed),
    (TaskStatus::Failed, VerifyStatus::TimedOut, true, TaskOutcome::Failed),
    (TaskStatus::Skipped, VerifyStatus::Pending, false, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Pending, true, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Passed, false, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Passed, true, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Failed, false, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Failed, true, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Skipped, false, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::Skipped, true, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::TimedOut, false, TaskOutcome::Skipped),
    (TaskStatus::Skipped, VerifyStatus::TimedOut, true, TaskOutcome::Skipped),
    (TaskStatus::Stopped, VerifyStatus::Pending, false, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Pending, true, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Passed, false, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Passed, true, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Failed, false, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Failed, true, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Skipped, false, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::Skipped, true, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::TimedOut, false, TaskOutcome::Stopped),
    (TaskStatus::Stopped, VerifyStatus::TimedOut, true, TaskOutcome::Stopped),
];

/// `ALL` is a fixed-size array, so a new enum variant does not break it and the
/// golden table would silently stop covering the whole product. These matches go
/// non-exhaustive the moment a variant is added, which drags whoever added it
/// into this file — where the next step is to extend `ALL` and the table.
#[test]
fn all_lists_stay_in_step_with_their_enums() {
    for status in TaskStatus::ALL {
        match status {
            TaskStatus::Waiting
            | TaskStatus::Pending
            | TaskStatus::Running
            | TaskStatus::AwaitingInput
            | TaskStatus::Stalled
            | TaskStatus::Done
            | TaskStatus::Merged
            | TaskStatus::Failed
            | TaskStatus::Skipped
            | TaskStatus::Stopped => {}
        }
    }
    for verify_status in VerifyStatus::ALL {
        match verify_status {
            VerifyStatus::Pending
            | VerifyStatus::Passed
            | VerifyStatus::Failed
            | VerifyStatus::Skipped
            | VerifyStatus::TimedOut => {}
        }
    }
    assert_eq!(TaskStatus::ALL.len(), 10);
    assert_eq!(VerifyStatus::ALL.len(), 5);
}

#[test]
fn golden_cartesian_derivation_table() {
    let expected_cell_count = TaskStatus::ALL.len() * VerifyStatus::ALL.len() * 2;
    assert_eq!(GOLDEN_TABLE.len(), expected_cell_count);

    for status in TaskStatus::ALL {
        for verify_status in VerifyStatus::ALL {
            for verify_required in [false, true] {
                let matches = GOLDEN_TABLE.iter().filter(|cell| {
                    cell.0 == status && cell.1 == verify_status && cell.2 == verify_required
                });
                let cells: Vec<_> = matches.collect();
                assert_eq!(
                    cells.len(),
                    1,
                    "golden table must contain exactly one cell for status={}, verify_status={}, verify_required={verify_required}",
                    status.as_str(),
                    verify_status.as_str(),
                );
                assert_eq!(
                    TaskOutcome::derive(status, verify_status, verify_required),
                    cells[0].3,
                    "status={}, verify_status={}, verify_required={verify_required}",
                    status.as_str(),
                    verify_status.as_str(),
                );
            }
        }
    }
}

#[test]
fn success_and_unverified_predicates_are_explicit() {
    assert!(TaskOutcome::Verified.is_success());
    assert!(TaskOutcome::Delivered.is_success());
    assert!(!TaskOutcome::Unverified(UnverifiedReason::NoResult).is_success());
    assert!(!TaskOutcome::Broken.is_success());
    assert!(!TaskOutcome::Failed.is_success());
    assert!(!TaskOutcome::Stopped.is_success());
    assert!(!TaskOutcome::Skipped.is_success());
    assert!(!TaskOutcome::InProgress.is_success());

    assert!(TaskOutcome::Unverified(UnverifiedReason::TimedOut).is_unverified());
    assert!(TaskOutcome::Unverified(UnverifiedReason::Infrastructure).is_unverified());
    assert!(TaskOutcome::Unverified(UnverifiedReason::NoResult).is_unverified());
    assert!(!TaskOutcome::Verified.is_unverified());
    assert!(!TaskOutcome::Broken.is_unverified());
}

#[test]
fn report_strings_keep_outcome_and_verification_tag_vocabularies_stable() {
    assert_eq!(TaskOutcome::Verified.as_str(), "verified");
    assert_eq!(TaskOutcome::Delivered.as_str(), "delivered");
    assert_eq!(TaskOutcome::Unverified(UnverifiedReason::TimedOut).as_str(), "unverified");
    assert_eq!(TaskOutcome::Broken.as_str(), "broken");
    assert_eq!(TaskOutcome::InProgress.as_str(), "in_progress");
    assert_eq!(TaskOutcome::Broken.verification_tag(), Some("VFAIL"));
    assert_eq!(
        TaskOutcome::Unverified(UnverifiedReason::TimedOut).verification_tag(),
        Some("VTIMEOUT")
    );
    assert_eq!(
        TaskOutcome::Unverified(UnverifiedReason::Infrastructure).verification_tag(),
        Some("VINFRA")
    );
    assert_eq!(
        TaskOutcome::Unverified(UnverifiedReason::NoResult).verification_tag(),
        Some("VNORESULT")
    );
    assert_eq!(TaskOutcome::Delivered.verification_tag(), None);
}

#[test]
fn merge_candidates_include_only_delivered_outcomes() {
    assert!(TaskOutcome::Verified.is_merge_candidate());
    assert!(TaskOutcome::Delivered.is_merge_candidate());
    assert!(TaskOutcome::Unverified(UnverifiedReason::NoResult).is_merge_candidate());
    assert!(TaskOutcome::Broken.is_merge_candidate());
    assert!(!TaskOutcome::Failed.is_merge_candidate());
    assert!(!TaskOutcome::Stopped.is_merge_candidate());
    assert!(!TaskOutcome::Skipped.is_merge_candidate());
    assert!(!TaskOutcome::InProgress.is_merge_candidate());
}

#[test]
fn verify_required_reads_the_verify_column_contract() {
    assert!(!verify_required(None));
    assert!(!verify_required(Some("")));
    assert!(!verify_required(Some("none")));
    assert!(!verify_required(Some("false")));
    assert!(!verify_required(Some("skip")));
    assert!(!verify_required(Some(" none ")));
    assert!(!verify_required(Some(" false ")));
    assert!(!verify_required(Some(" skip ")));
    assert!(verify_required(Some("true")));
    assert!(verify_required(Some(" true ")));
    assert!(verify_required(Some(" auto ")));
    assert!(verify_required(Some(" cargo test ")));
}
