// prompt tests.
// Uses the shared test fixtures and imports from the parent test module.

use super::*;

#[test]
fn stores_recent_question_as_awaiting_prompt_metadata() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = Task {
        id: TaskId("t-pty1".to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Running,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        project_id: None,
        worktree_path: None,
        effective_dir: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None,
        observed_model: None,
        attribution_source: None,
        cost_usd: None,
        exit_code: None,
        created_at: Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Skipped,
        pending_reason: None,
        read_only: false,
        budget: false,
        audit_verdict: None,
        audit_report_path: None,
        delivery_assessment: None,
    };
    store.insert_task(&task).unwrap();

    let prompt = "115:    use super::board::render_board;";
    let awaiting_prompt = extract_awaiting_prompt(
        "Should I update board.rs?\n115:    use super::board::render_board;",
        prompt,
    );
    let mut awaiting_input = false;
    mark_awaiting_input(
        &store,
        &task.id,
        prompt,
        &awaiting_prompt,
        &mut awaiting_input,
    )
    .unwrap();

    let event = store.get_events(task.id.as_str()).unwrap().pop().unwrap();
    assert_eq!(event.detail, prompt);
    assert_eq!(
        event
            .metadata
            .as_ref()
            .and_then(|m| m.get("awaiting_prompt"))
            .and_then(|v| v.as_str()),
        Some("Should I update board.rs?")
    );
}

#[test]
fn handles_ansi_escaped_output() {
    let input = "\x1b[1mShould I proceed?\x1b[0m";
    let prompt = "fallback";
    let result = extract_awaiting_prompt(input, prompt);
    assert_eq!(result, "Should I proceed?");
}

#[test]
fn finds_question_beyond_six_lines() {
    let mut output = String::new();
    for i in 0..15 {
        output.push_str(&format!("Line {}\n", i));
    }
    output.push_str("What about this file?\n");
    output.push_str("116:    use super::other;\n");

    let result = extract_awaiting_prompt(&output, "fallback");
    assert_eq!(result, "What about this file?");
}

#[test]
fn matches_patterns_without_question_mark() {
    let output = "Do you want to continue\n1: code here";
    let result = extract_awaiting_prompt(output, "fallback");
    assert_eq!(result, "Do you want to continue");
}

#[test]
fn falls_back_to_prompt_when_no_question() {
    let output = "Some random output\nNo question here\nJust code lines";
    let result = extract_awaiting_prompt(output, "fallback prompt");
    assert_eq!(result, "fallback prompt");
}

#[test]
fn handles_osc_escaped_output() {
    let input = "\x1b]0;⛬ window title\x07\x1b[1mShould I proceed?\x1b[0m";
    let result = extract_awaiting_prompt(input, "fallback");
    assert_eq!(result, "Should I proceed?");
}
