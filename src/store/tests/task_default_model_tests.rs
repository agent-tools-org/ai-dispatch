// Additional default-model history regressions.
// Deps: parent store test fixtures.

use super::*;

/// A Done task with hollow delivery is not a success. Learning a default model
/// from it would route future work based on a run that produced nothing.
#[test]
fn latest_default_model_skips_hollow_delivery_done_tasks() {
    let store = Store::open_memory().unwrap();
    let now = chrono::Local::now();

    let mut hollow = make_task("t-hollow-new", AgentKind::Codex, TaskStatus::Done);
    hollow.created_at = now;
    hollow.observed_model = Some("gpt-hollow".to_string());
    hollow.attribution_source = Some(AttributionSource::Echoed);
    hollow.delivery_assessment = Some(DeliveryAssessment::HollowOutput);

    let mut good = make_task("t-good-old", AgentKind::Codex, TaskStatus::Done);
    good.created_at = now - chrono::Duration::hours(2);
    good.observed_model = Some("gpt-good".to_string());
    good.attribution_source = Some(AttributionSource::Echoed);

    store.insert_task(&hollow).unwrap();
    store.insert_task(&good).unwrap();

    assert_eq!(
        store
            .latest_default_model(AgentKind::Codex)
            .unwrap()
            .as_deref(),
        Some("gpt-good")
    );
}
