// Tests for checklist file loading, prompt injection, and background serde.
// Covers `aid run --checklist` foundation behavior without touching agent execution.
// Deps: cmd::checklist helpers, run_prompt bundle builder, background spec serde.

use super::*;
use crate::background::BackgroundRunSpec;
use crate::cmd::checklist::merge_checklist_items;
use crate::store::Store;

fn prompt_args(checklist: Vec<String>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Write the requested content".to_string(),
        checklist,
        ..Default::default()
    }
}

fn background_spec(checklist: Vec<String>) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: "t-check".to_string(),
        worker_pid: Some(1),
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: None,
        output: None,
        model: None,
        verify: None,
        judge: None,
        max_duration_mins: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist,
        template: None,
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        container: None,
    }
}

#[test]
fn checklist_items_are_injected_into_prompt_with_required_format() {
    let store = Store::open_memory().unwrap();
    let bundle = run_prompt::build_prompt_bundle(
        &store,
        &prompt_args(vec![
            "Verify auth flow handles retries".to_string(),
            "Confirm tests cover failure cases".to_string(),
        ]),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    let expected = concat!(
        "<aid-checklist>\n",
        "MANDATORY CHECKLIST — You MUST explicitly address EVERY item below.\n",
        "For each item, state CONFIRMED (with evidence) or REJECTED (with reasoning).\n",
        "Do NOT skip any item. Missing responses will trigger an automatic retry.\n\n",
        "[ ] 1. Verify auth flow handles retries\n",
        "[ ] 2. Confirm tests cover failure cases\n",
        "</aid-checklist>",
    );
    assert!(bundle.effective_prompt.contains(expected));
}

#[test]
fn checklist_file_loading_skips_comments_and_blank_lines() {
    let file = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        file.path(),
        "# comment\n\nfirst item\n  second item  \n   # ignored\n",
    )
    .unwrap();

    let checklist = merge_checklist_items(
        vec!["inline item".to_string()],
        Some(file.path().to_str().unwrap()),
    )
    .unwrap();

    assert_eq!(
        checklist,
        vec![
            "inline item".to_string(),
            "first item".to_string(),
            "second item".to_string(),
        ]
    );
}

#[test]
fn background_run_spec_round_trips_checklist_via_serde() {
    let value = serde_json::to_value(background_spec(vec![
        "confirm branch state".to_string(),
        "verify retry wiring".to_string(),
    ]))
    .unwrap();
    let decoded: BackgroundRunSpec = serde_json::from_value(value).unwrap();

    assert_eq!(
        decoded.checklist,
        vec![
            "confirm branch state".to_string(),
            "verify retry wiring".to_string(),
        ]
    );
}

#[test]
fn empty_checklist_produces_no_prompt_injection() {
    let store = Store::open_memory().unwrap();
    let bundle = run_prompt::build_prompt_bundle(
        &store,
        &prompt_args(vec![]),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("<aid-checklist>"));
}
