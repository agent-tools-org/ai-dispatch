// Tests that `aid agent list --json` reports evidence states, not assumptions.
// Covers: claude listed, grok auth failed/unknown, installed from the route predicate.
// Deps: agent_json builders, auth_marker, isolated AID home, DetectAgentsGuard.

use super::get_agents_list;
use crate::types::AgentKind;

fn isolated() -> (tempfile::TempDir, crate::paths::AidHomeGuard, crate::live_quota::CacheDirGuard) {
    let tmp = tempfile::tempdir().expect("tempdir");
    let guard = crate::paths::AidHomeGuard::set(tmp.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).ok();
    let aidbar = tmp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).ok();
    let cache = crate::live_quota::CacheDirGuard::set(&aidbar);
    (tmp, guard, cache)
}

#[test]
fn agent_list_includes_claude_and_reports_unknown_without_evidence() {
    let (_tmp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude]);
    let store = crate::store::Store::open_memory().expect("store");
    let list = get_agents_list(&store).expect("agent list");
    let claude = list.agents.iter().find(|agent| agent.name == "claude").expect("claude listed");
    assert!(claude.installed);
    let grok = list.agents.iter().find(|agent| agent.name == "grok").expect("grok listed");
    assert!(!grok.installed);
    assert_eq!(grok.quota.source, "none");
    assert_eq!(grok.quota.state, "unknown");
    assert_eq!(grok.auth.state, "unknown");
    let value = serde_json::to_value(grok).expect("json");
    assert_eq!(value["auth"]["state"], "unknown");
    assert!(value["auth"].get("observed_at").is_none());
}

#[test]
fn agent_list_reports_failed_auth_with_observed_time() {
    let (_tmp, _home, _cache) = isolated();
    crate::auth_marker::record_failure_at(AgentKind::Grok, "Not signed in.", chrono::Local::now());
    let store = crate::store::Store::open_memory().expect("store");
    let list = get_agents_list(&store).expect("agent list");
    let grok = list.agents.iter().find(|agent| agent.name == "grok").expect("grok listed");
    assert_eq!(grok.auth.state, "failed");
    assert!(grok.auth.observed_at.is_some());
    let codex = list.agents.iter().find(|agent| agent.name == "codex").expect("codex listed");
    assert_eq!(codex.auth.state, "unknown");
}
