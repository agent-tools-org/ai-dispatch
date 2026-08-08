// Tests for batch auto-fallback helpers.
// Covers one-shot retry gating and fallback agent resolution from stored tasks.
// Deps: crate::cmd::batch, crate::store::Store, crate::types.

use chrono::Local;
use tempfile::TempDir;

use super::batch::{
    auto_fallback_agent,
    pre_dispatch_fallback_choice,
    should_auto_fallback,
    BatchTaskOutcome,
};
use crate::paths::AidHomeGuard;
use crate::rate_limit::{clear_rate_limit, mark_rate_limited};
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};

fn stored_task(id: &str, agent: AgentKind) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Failed,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        final_head_sha: None,
        final_branch: None,
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        requested_model: None, observed_model: None, attribution_source: None,
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

fn isolated_rate_limit_home() -> (TempDir, AidHomeGuard) {
    let temp_dir = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp_dir.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).unwrap();
    (temp_dir, guard)
}

fn dispatch_agent_name(agent_name: &str, fallback: Option<&str>) -> String {
    match pre_dispatch_fallback_choice(agent_name, fallback).unwrap() {
        Some((fallback_agent, _)) => fallback_agent,
        None => agent_name.to_string(),
    }
}

#[test]
fn should_auto_fallback_only_once_for_failed_tasks() {
    assert!(should_auto_fallback(true, false, BatchTaskOutcome::Failed));
    assert!(!should_auto_fallback(true, true, BatchTaskOutcome::Failed));
    assert!(!should_auto_fallback(true, false, BatchTaskOutcome::Done));
    assert!(!should_auto_fallback(false, false, BatchTaskOutcome::Failed));
}

#[test]
fn auto_fallback_agent_returns_none_when_no_usable_peer() {
    let store = Store::open_memory().unwrap();
    // Only the exhausted agent is installed — category-aware fallback must not invent peers.
    let _agents = crate::agent::DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
    store.insert_task(&stored_task("t-mimocode", AgentKind::MiMoCode)).unwrap();

    assert!(auto_fallback_agent(&store, "t-mimocode", &[], 0).unwrap().is_none());
}

#[test]
fn pre_dispatch_uses_fallback_when_agent_is_rate_limited() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");

    let choice = pre_dispatch_fallback_choice("codex", Some("opencode,cursor"))
        .unwrap()
        .expect("fallback choice");

    assert_eq!(dispatch_agent_name("codex", Some("opencode,cursor")), "opencode");
    assert_eq!(choice.0, "opencode");
    assert_eq!(choice.1, vec!["cursor".to_string()]);

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn pre_dispatch_keeps_original_when_no_fallback_is_available() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");

    assert_eq!(dispatch_agent_name("codex", None), "codex");
    assert!(pre_dispatch_fallback_choice("codex", None).unwrap().is_none());

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn auto_fallback_skips_rate_limited_toml_fallbacks() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::OpenCode, None, "rate limit exceeded");

    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-codex", AgentKind::Codex)).unwrap();
    let tasks = vec![batch_task_with_fallback("opencode,cursor")];

    let result = auto_fallback_agent(&store, "t-codex", &tasks, 0).unwrap();
    assert!(result.is_some());
    let (original, fallback) = result.unwrap();
    assert_eq!(original, "codex");
    assert_eq!(fallback, "cursor");

    clear_rate_limit(&AgentKind::OpenCode, None);
}

fn write_custom_agent(name: &str) {
    let agents_dir = crate::paths::aid_dir().join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{name}.toml")),
        format!("[agent]\nid = \"{name}\"\ndisplay_name = \"{name}\"\ncommand = \"{name}\"\n"),
    )
    .unwrap();
}

fn batch_task_with_fallback(fallback: &str) -> crate::batch::BatchTask {
    crate::batch::BatchTask {
        id: None,
        name: None,
        agent: String::new(),
        team: None,
        prompt: String::new(),
        prompt_file: None,
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        peer_review: None,
        best_of: None,
        max_duration_mins: None,
        max_wait_mins: None,
        retry: None,
        idle_timeout: None,
        metric: None,
        context: None,
        checklist: None,
        skills: None,
        on_done: None,
        hooks: None,
        depends_on: None,
        parent: None,
        context_from: None,
        fallback: Some(fallback.to_string()),
        scope: None,
        read_only: false,
        sandbox: false,
        no_skill: false,
        difficulty: None,
        budget: None,
        urgency: None,
        rigor: None,
        egress: None,
        kind: None,
        audit: None,
        env: None,
        env_forward: None,
        worktree_link_deps: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }
}

/// Custom agents named in batch fallback must be selected — same rule as
/// `aid run` cascade parsing, not silently dropped by AgentKind::parse_str.
#[test]
fn pre_dispatch_uses_custom_agent_in_fallback() {
    let (_temp, _guard) = isolated_rate_limit_home();
    write_custom_agent("glm5");
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");

    let choice = pre_dispatch_fallback_choice("codex", Some("glm5,cursor"))
        .unwrap()
        .expect("custom fallback");

    assert_eq!(choice.0, "glm5");
    assert_eq!(choice.1, vec!["cursor".to_string()]);

    clear_rate_limit(&AgentKind::Codex, None);
}

/// An unresolvable batch fallback entry must surface as an error, never be
/// skipped the way filter_map(AgentKind::parse_str) used to.
#[test]
fn pre_dispatch_unknown_fallback_is_an_error() {
    let (_temp, _guard) = isolated_rate_limit_home();
    mark_rate_limited(&AgentKind::Codex, None, "rate limit exceeded");

    let err = pre_dispatch_fallback_choice("codex", Some("not-a-real-agent"))
        .expect_err("unknown cascade agent must error");
    assert!(
        err.to_string().contains("not-a-real-agent"),
        "error must name the unknown agent: {err}"
    );

    clear_rate_limit(&AgentKind::Codex, None);
}

#[test]
fn auto_fallback_agent_selects_custom_toml_fallback() {
    let (_temp, _guard) = isolated_rate_limit_home();
    write_custom_agent("glm5");

    let store = Store::open_memory().unwrap();
    store.insert_task(&stored_task("t-codex", AgentKind::Codex)).unwrap();
    let tasks = vec![batch_task_with_fallback("glm5")];

    let result = auto_fallback_agent(&store, "t-codex", &tasks, 0).unwrap();
    let (original, fallback) = result.expect("custom fallback");
    assert_eq!(original, "codex");
    assert_eq!(fallback, "glm5");
}
