// Launch policy tests through real advice, inventory, and quota evidence.
// Covers: Gemini supersession, recommendation fallback, Claude preference, and eligibility.
// Deps: advise test isolation helpers, DetectAgentsGuard, auth markers.

use super::*;

#[test]
fn installed_agy_supersedes_gemini_research_advice() {
    let (_temp, _home, _cache) = isolated();
    for agy_installed in [true, false] {
        let mut fleet = vec![AgentKind::Gemini, AgentKind::Codex];
        if agy_installed { fleet.push(AgentKind::Antigravity); }
        let _fleet = crate::agent::DetectAgentsGuard::set(fleet);
        let report = advise("compare the docs", declared(TaskDifficulty::Moderate, TaskBudget::Standard),
            Some(TaskCategory::Research), None, None, 0, None);
        let gemini = find(&report, "gemini");
        assert_eq!(gemini.eligible, !agy_installed);
        assert_eq!(gemini.exclusion_codes.iter().any(|code| code == "superseded_by_agy"), agy_installed);
        if agy_installed {
            assert_eq!(gemini.exclusion_reason.as_deref(), Some("gemini individual tier superseded by agy (installed)"));
            assert!(!gemini.launchable(None));
            assert_ne!(report.recommended.expect("recommendation").agent, "gemini");
        }
    }
}

#[test]
fn superseded_gemini_is_never_a_recommendation_fallback() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Gemini, AgentKind::Antigravity, AgentKind::Codex]);
    for kind in [AgentKind::Antigravity, AgentKind::Codex] {
        crate::auth_marker::record_failure_at(kind, "Not signed in.", chrono::Local::now());
    }
    let report = advise("compare the docs", declared(TaskDifficulty::Moderate, TaskBudget::Standard),
        Some(TaskCategory::Research), None, None, 0, None);
    assert!(report.candidates.iter().all(|candidate| !candidate.eligible));
    assert_ne!(report.recommended.as_ref().map(|candidate| candidate.agent.as_str()), Some("gemini"));
}

#[test]
fn launchable_requires_eligibility_and_claude_team_preference() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude, AgentKind::Codex]);
    let report = run(None);
    let preferred: TeamConfig = toml::from_str("id = 'preferred'\ndisplay_name = 'Preferred'\npreferred_agents = ['claude']\n").expect("team");
    let other: TeamConfig = toml::from_str("id = 'other'\ndisplay_name = 'Other'\npreferred_agents = ['codex']\n").expect("team");
    let claude = find(&report, "claude");
    assert!(claude.eligible);
    assert_eq!(claude.kind(), Some(AgentKind::Claude));
    for (team, allowed) in [(None, false), (Some(&other), false), (Some(&preferred), true)] {
        assert_eq!(claude.launchable(team), allowed);
        assert!(find(&report, "codex").launchable(team));
        assert!(!find(&report, "grok").launchable(team));
    }
    let mut unknown = claude.clone();
    unknown.agent = "no-such-agent".to_string();
    assert_eq!(unknown.kind(), None);
    assert!(!unknown.launchable(Some(&preferred)));
}
