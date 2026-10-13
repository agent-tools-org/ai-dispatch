// Informational post-run notices must preserve the existing terminal failure reason.
// Exercises scope warnings and unavailable delivery recovery after settlement.
// Deps: parent settlement fixtures and existing post-run callbacks.

use super::*;

#[test]
fn scope_notice_preserves_settlement_winner() {
    let _permit = crate::test_subprocess::acquire();
    let f = Fixture::new(TaskStatus::Stopped);
    init_repo(f.home.path());
    std::fs::write(f.home.path().join("file.txt"), "outside scope\n").expect("edit");
    crate::cmd::run::run_agent::check_scope_violations(
        &f.store,
        &f.id,
        &["allowed.txt".into()],
        f.home.path().to_str(),
    );
    f.assert_unchanged();
    let events = f.store.get_events(f.id.as_str()).expect("events");
    assert!(events.iter().any(|event| {
        event.detail.starts_with("Scope violation:") && event.event_kind == EventKind::Milestone
    }));
}

#[tokio::test]
async fn unavailable_recovery_notice_preserves_settlement_winner() {
    let f = Fixture::new(TaskStatus::Failed);
    f.store
        .db()
        .execute(
            "UPDATE tasks SET read_only = 1 WHERE id = ?1",
            rusqlite::params![f.id.as_str()],
        )
        .expect("read-only fixture");
    f.store
        .update_delivery_assessment(
            f.id.as_str(),
            Some(DeliveryAssessment::MissingFinalDelivery),
        )
        .expect("recovery candidate");
    assert!(
        crate::cmd::run::run_delivery_recovery::maybe_auto_recover_missing_delivery(
            &f.store,
            &f.id,
            &RunArgs::default(),
        )
        .await
        .expect("recovery notice")
        .is_none()
    );
    f.assert_unchanged();
    let events = f.store.get_events(f.id.as_str()).expect("events");
    let event = events
        .iter()
        .find(|event| event.detail.contains("session ID unavailable"))
        .expect("unavailable notice visible");
    assert_eq!(event.event_kind, EventKind::Milestone);
    assert_eq!(
        event.metadata.as_ref().expect("metadata")["delivery_recovery"],
        "unavailable"
    );
}
