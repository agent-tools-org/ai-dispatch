// Exhaustive contract tests for task outcome derivation and verification rules.
// Exports: none; loaded by outcome.rs under `#[cfg(test)]`.
// Deps: crate::types public outcome and status re-exports.

use crate::types::{DeliveryAssessment, TaskOutcome, outcome::UnverifiedReason, verify_required};

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
fn empty_everywhere_delivery_demotes_success_to_failed() {
    // HollowOutput = nothing on any channel + no worktree changes; not mere quiet stdout.
    let hollow = Some(DeliveryAssessment::HollowOutput);
    assert_eq!(
        TaskOutcome::Delivered.with_delivery_assessment(hollow),
        TaskOutcome::Failed
    );
    assert_eq!(
        TaskOutcome::Verified.with_delivery_assessment(hollow),
        TaskOutcome::Failed
    );
    assert_eq!(
        TaskOutcome::Delivered.with_delivery_assessment(Some(DeliveryAssessment::MissingFinalDelivery)),
        TaskOutcome::Failed
    );
    // EmptyDiff alone may still be a real delivery (report file / commit-only).
    assert_eq!(
        TaskOutcome::Delivered.with_delivery_assessment(Some(DeliveryAssessment::EmptyDiff)),
        TaskOutcome::Delivered
    );
    assert_eq!(
        TaskOutcome::Verified.with_delivery_assessment(None),
        TaskOutcome::Verified
    );
    assert_eq!(
        TaskOutcome::Broken.with_delivery_assessment(hollow),
        TaskOutcome::Broken
    );
    assert_eq!(
        TaskOutcome::Unverified(UnverifiedReason::NoResult).with_delivery_assessment(hollow),
        TaskOutcome::Unverified(UnverifiedReason::NoResult)
    );
}

#[test]
fn report_strings_keep_outcome_and_verification_tag_vocabularies_stable() {
    assert_eq!(TaskOutcome::Verified.as_str(), "verified");
    assert_eq!(TaskOutcome::Delivered.as_str(), "delivered");
    assert_eq!(
        TaskOutcome::Unverified(UnverifiedReason::TimedOut).as_str(),
        "unverified"
    );
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
