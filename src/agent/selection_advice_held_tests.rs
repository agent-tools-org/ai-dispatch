// Held-route recommendation regression coverage.
// Uses isolated advice fixtures and route-wide refusal markers.

use super::*;

#[test]
fn needs_human_route_is_never_recommended_even_for_background() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Grok]);
    let marker = crate::routing::rate_limit::marker_path(&AgentKind::Grok, None);
    std::fs::create_dir_all(marker.parent().expect("parent")).expect("aid dir");
    std::fs::write(
        marker,
        "hold: manual\nmessage: API error (status 402 Payment Required)\n",
    )
    .expect("marker");
    for urgency in [TaskUrgency::Normal, TaskUrgency::Background] {
        let mut profile = declared(TaskDifficulty::Moderate, TaskBudget::Standard);
        profile.urgency = urgency;
        let report = advise("refactor scheduler", profile, None, None, None, 0, None);
        let grok = find(&report, "grok");
        assert_eq!(grok.quota.status, "held");
        assert!(!grok.eligible);
        assert!(grok.exclusion_codes.iter().any(|code| code == "held"));
        assert!(report.recommended.is_none());
    }
}
