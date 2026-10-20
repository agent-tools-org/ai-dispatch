// Launch policy tests through real advice, inventory, and quota evidence.
// Covers: Claude preference and eligibility.
// Deps: advise test isolation helpers, DetectAgentsGuard, auth markers.

use super::*;

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
