// Focused Claude selection tests to keep the legacy selection test file from growing further.
// Exports: module-scoped tests only.
// Deps: super::select_agent_from, selection_scoring::base_score, Store.

use super::select_agent_from;
use super::selection_scoring::base_score;
use crate::agent::RunOpts;
use crate::agent::classifier::TaskCategory;
use crate::store::Store;
use crate::types::AgentKind;

fn opts() -> RunOpts {
    RunOpts {
        dir: Some("src".to_string()),
        output: None,
        model: None,
        budget: false,
        read_only: false,
        context_files: vec![],
        session_id: None,
        env: None,
        env_forward: None,
    }
}

#[test]
fn claude_capability_scores_match_rebalance() {
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::ComplexImpl), 10);
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::Testing), 10);
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::Research), 9);
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::Documentation), 9);
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::Debugging), 10);
    assert_eq!(base_score(AgentKind::Claude, TaskCategory::Refactoring), 10);
}

#[test]
fn auto_selection_can_choose_claude() {
    let store = Store::open_memory().unwrap();
    let prompt = "Implement a typed task planner across src/agent/selection.rs and src/types.rs";
    let (kind, reason) = select_agent_from(
        prompt,
        &opts(),
        &[AgentKind::Claude, AgentKind::Cursor],
        &store,
        None,
    );
    assert_eq!(kind, AgentKind::Claude.as_str());
    assert!(reason.contains("claude"));
}

#[test]
fn fallback_chains_place_claude_correctly() {
    let coding = super::next_fallback_in_chain(
        &AgentKind::Codex,
        super::CODING_FALLBACK_CHAIN,
        &[AgentKind::Claude, AgentKind::Cursor],
    );
    assert_eq!(coding, Some(AgentKind::Claude));

    let research = super::next_fallback_in_chain(
        &AgentKind::Gemini,
        super::RESEARCH_FALLBACK_CHAIN,
        &[AgentKind::Claude, AgentKind::Codex],
    );
    assert_eq!(research, Some(AgentKind::Claude));
}
