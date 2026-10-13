// Context display regressions and shared show task fixtures.
// Uses stored launch text; legacy rows expose only their stored raw brief.
use super::*;

pub(in crate::cmd::show) fn task_fixture(
    id: &str,
    prompt: &str,
    resolved_prompt: Option<&str>,
    repo_path: Option<String>,
) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: prompt.to_string(),
        resolved_prompt: resolved_prompt.map(String::from),
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path,
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
    }
}

pub(super) fn research_task(task_id: &str, repo: &Path) -> Task {
    task_fixture(
        task_id,
        "research task",
        None,
        Some(repo.display().to_string()),
    )
}

#[test]
fn context_text_prefers_stored_resolved_prompt() {
    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-context", "raw prompt", Some("resolved prompt"), None);
    store.insert_task(&task).unwrap();

    let text = context_text(&store, "t-context").unwrap();

    assert!(text.contains("=== Original Prompt ===\nraw prompt"));
    assert!(text.contains("=== Resolved Prompt ===\nresolved prompt"));
    assert!(!text.contains("(reconstructed"));
}

// Saved skill names cannot recreate missing dispatch context.
#[test]
fn context_text_reports_missing_resolved_prompt_without_loading_skills() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-reconstruct", "raw prompt", None, None);
    store.insert_task(&task).unwrap();
    store
        .update_task_dispatch_args("t-reconstruct", r#"{"skills":["implementer"]}"#)
        .unwrap();

    let text = context_text(&store, "t-reconstruct").unwrap();

    assert!(text.contains("=== Original Prompt ===\nraw prompt"));
    assert!(text.contains("=== Resolved Prompt ===\n(not stored for this task)"));
    assert!(!text.contains("# Implementer"));
    assert!(!text.contains("[MILESTONE] <brief description>"));
    assert!(!text.contains("reconstructed"));
}

/// A task dispatched with no skills reports none, rather than having one
/// invented for it from the agent kind.
#[test]
fn context_text_reports_no_skills_when_none_were_dispatched() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-noskill", "raw prompt", None, None);
    store.insert_task(&task).unwrap();

    let text = context_text(&store, "t-noskill").unwrap();

    assert!(
        !text.contains("# Implementer"),
        "no skill was dispatched, none may be reported"
    );
}

#[test]
fn summary_text_shows_conclusion_for_research_task() {
    let temp = tempfile::tempdir().unwrap();
    init_git_repo(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let task = research_task("t-summary-conclusion", temp.path());
    store.insert_task(&task).unwrap();
    save_completion_summary(&store, task.id.as_str(), "Summarized the research outcome.");

    let text = summary_text(&store, task.id.as_str()).unwrap();

    assert!(text.contains("--- Diff Stat ---"));
    assert!(text.contains("(no changes detected)"));
    assert!(text.contains("Conclusion: Summarized the research outcome."));
}
