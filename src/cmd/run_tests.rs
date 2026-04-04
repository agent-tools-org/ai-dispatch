// Tests for the `run` command module after splitting from run.rs.
// Covers dispatch validation, quota detection, cascade behavior, and dry-run flow.
// Depends on the parent run module, store, paths, tokio, and tempfile.
use super::*;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskStatus, VerifyStatus};
use std::process::Command;
use std::sync::Arc;
use tempfile::TempDir;

fn git(dir: &std::path::Path, args: &[&str]) {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git command failed");
    assert!(output.status.success(), "git {:?} failed: {}", args, String::from_utf8_lossy(&output.stderr));
}

#[test]
fn empty_diff_detection_respects_worktree_state() {
    let dir = TempDir::new().unwrap();
    git(dir.path(), &["init"]);
    git(dir.path(), &["config", "user.email", "aid@example.com"]);
    git(dir.path(), &["config", "user.name", "Aid Tester"]);
    let file = dir.path().join("file.txt");
    std::fs::write(&file, "initial").unwrap();
    git(dir.path(), &["add", "file.txt"]);
    git(dir.path(), &["commit", "-m", "initial"]);
    assert_eq!(worktree_is_empty_diff(dir.path()), Some(true));
    std::fs::write(&file, "updated").unwrap();
    assert_eq!(worktree_is_empty_diff(dir.path()), Some(false));
}

#[test]
fn take_next_cascade_agent_consumes_first_entry() {
    let args = RunArgs {
        agent_name: "primary".to_string(),
        cascade: vec!["codex".to_string(), "cursor".to_string()],
        ..Default::default()
    };
    let result = take_next_cascade_agent(&args);
    assert_eq!(result, Some(("codex".to_string(), vec!["cursor".to_string()])));
}

#[test]
fn take_next_cascade_agent_returns_none_when_empty() {
    let args = RunArgs { cascade: vec![], ..Default::default() };
    assert!(take_next_cascade_agent(&args).is_none());
}

#[test]
fn read_quota_error_message_uses_stderr() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-quota-stderr"),
        "You have exhausted your capacity for today.",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-stderr".to_string()));
    assert_eq!(message.as_deref(), Some("You have exhausted your capacity for today."));
}

#[test]
fn read_quota_error_message_falls_back_to_log() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::log_path("t-quota-log"),
        "{\"error\":\"You have hit your usage limit.\"}\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-log".to_string()));
    assert_eq!(message.as_deref(), Some("{\"error\":\"You have hit your usage limit.\"}"));
}

#[test]
fn read_quota_error_message_extracts_rate_limit_line_only() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-quota-mixed"),
        "tokens: 8714294 in + 27373 out = 8741667 (8442752 cached)\nYou have exhausted your capacity for today.\nsome other line\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-mixed".to_string()));
    assert_eq!(message.as_deref(), Some("You have exhausted your capacity for today."));
}

#[test]
fn read_quota_error_message_detects_402_payment_errors() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::log_path("t-quota-402"),
        "{\"type\":\"error\",\"source\":\"agent_loop\",\"message\":\"402 payment required: reload your tokens\"}\n",
    )
    .unwrap();
    let message = read_quota_error_message(&TaskId("t-quota-402".to_string()));
    assert_eq!(message.as_deref(), Some("402 payment required: reload your tokens"));
}

#[test]
fn rescue_quota_failed_task_marks_passed_verify_as_done() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-rescue-pass"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-rescue-pass");
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id).as_deref(),
    );
    let task = store.get_task("t-rescue-pass").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Done);
}

#[test]
fn rescue_quota_failed_task_keeps_failed_verify_failed() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();
    std::fs::write(
        paths::stderr_path("t-rescue-fail"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let task = make_failed_task("t-rescue-fail");
    store.insert_task(&task).unwrap();
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id).as_deref(),
    );
    let task = store.get_task("t-rescue-fail").unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
}

#[test]
fn validate_dispatch_warns_short_prompt() {
    assert_eq!(validate_dispatch(&RunArgs { prompt: "tiny".to_string(), ..Default::default() }, &AgentKind::Gemini), vec!["Prompt is very short, agent may not have enough context".to_string()]);
}

#[test]
fn validate_dispatch_warns_code_agent_without_dir() {
    assert_eq!(validate_dispatch(&RunArgs { prompt: "Implement the dispatcher".to_string(), ..Default::default() }, &AgentKind::Codex), vec!["Code agent without --dir may not be able to write files".to_string()]);
}

#[test]
fn resolve_prompt_input_reads_prompt_file() {
    let dir = TempDir::new().unwrap();
    let prompt_file = dir.path().join("prompt.md");
    std::fs::write(&prompt_file, "Prompt from file").unwrap();

    let prompt = resolve_prompt_input("", Some(prompt_file.to_str().unwrap())).unwrap();

    assert_eq!(prompt, "Prompt from file");
}

#[test]
fn resolve_prompt_input_rejects_prompt_and_prompt_file() {
    let err = resolve_prompt_input("inline prompt", Some("/tmp/prompt.md"))
        .unwrap_err()
        .to_string();

    assert_eq!(err, "Cannot use both --prompt and --prompt-file");
}

#[test]
fn resolve_prompt_input_requires_prompt_source() {
    let err = resolve_prompt_input("", None).unwrap_err().to_string();

    assert_eq!(err, "Either prompt or --prompt-file is required");
}

#[test]
fn sandboxed_agents_identified() {
    assert!(AgentKind::OpenCode.sandboxed_fs());
    assert!(!AgentKind::Codex.sandboxed_fs());
    assert!(!AgentKind::Gemini.sandboxed_fs());
}

#[test]
fn build_prompt_bundle_uses_relative_workspace_for_sandboxed_agents() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store.create_workgroup("batch", "desc", Some("seed"), None).unwrap();
    let workspace = crate::paths::workspace_dir(group.id.as_str()).unwrap();
    let bundle = run_prompt::build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::OpenCode,
        None,
        &[],
        "task-opencode",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("[Shared Workspace] Path: .aid-workspace"));
    assert!(!bundle.effective_prompt.contains(&workspace.display().to_string()));
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn build_prompt_bundle_keeps_absolute_workspace_for_non_sandboxed_agents() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store.create_workgroup("batch", "desc", Some("seed"), None).unwrap();
    let workspace = crate::paths::workspace_dir(group.id.as_str()).unwrap();
    let bundle = run_prompt::build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::Codex,
        None,
        &[],
        "task-codex",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains(&workspace.display().to_string()));
    assert!(!bundle.effective_prompt.contains("[Shared Workspace] Path: .aid-workspace"));
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn workspace_symlink_guard_creates_and_cleans_up_link() {
    let group_id = format!("wg-symlink-{:04x}", rand::random::<u16>());
    let workspace = crate::paths::workspace_dir(&group_id).unwrap();
    std::fs::create_dir_all(&workspace).unwrap();
    let work_dir = TempDir::new().unwrap();
    let link_path = work_dir.path().join(".aid-workspace");

    {
        let _guard = WorkspaceSymlinkGuard::create(
            AgentKind::OpenCode,
            Some(&group_id),
            work_dir.path().to_str(),
        )
        .unwrap();
        assert!(link_path.exists());
        assert_eq!(std::fs::read_link(&link_path).unwrap(), workspace);
    }

    assert!(!link_path.exists());
    let _ = std::fs::remove_dir_all(workspace);
}

#[test]
fn validate_dispatch_warns_long_prompt() {
    let prompt = "a".repeat(5001);
    assert_eq!(validate_dispatch(&RunArgs { prompt, ..Default::default() }, &AgentKind::Gemini), vec!["Very long prompt (5001 chars), consider using --context files instead".to_string()]);
}

#[test]
fn validate_dispatch_warns_research_worktree() {
    assert_eq!(validate_dispatch(&RunArgs { prompt: "valid prompt text".to_string(), worktree: Some("wt".to_string()), ..Default::default() }, &AgentKind::Gemini), vec!["Research agent with --worktree is unusual, did you mean a code agent?".to_string()]);
}
#[test]
fn resolve_id_conflict_none_for_missing_id() {
    let store = Store::open_memory().unwrap();
    assert!(matches!(resolve_id_conflict(&store, "new-task").unwrap(), IdConflict::None));
}

#[test]
fn resolve_id_conflict_replace_waiting() {
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("my-task");
    task.status = TaskStatus::Waiting;
    store.insert_task(&task).unwrap();
    assert!(matches!(resolve_id_conflict(&store, "my-task").unwrap(), IdConflict::ReplaceWaiting));
}

#[test]
fn resolve_id_conflict_blocks_running() {
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("my-task");
    task.status = TaskStatus::Running;
    store.insert_task(&task).unwrap();
    assert!(matches!(resolve_id_conflict(&store, "my-task").unwrap(), IdConflict::Running));
}

#[test]
fn resolve_id_conflict_auto_suffixes_terminal() {
    let store = Store::open_memory().unwrap();
    store.insert_task(&make_failed_task("my-task")).unwrap();
    match resolve_id_conflict(&store, "my-task").unwrap() {
        IdConflict::AutoSuffix(new_id) => assert_eq!(new_id, "my-task-2"),
        other => panic!("expected AutoSuffix, got {:?}", std::mem::discriminant(&other)),
    }
    // Insert my-task-2, should get my-task-3 next
    store.insert_task(&make_failed_task("my-task-2")).unwrap();
    match resolve_id_conflict(&store, "my-task").unwrap() {
        IdConflict::AutoSuffix(new_id) => assert_eq!(new_id, "my-task-3"),
        other => panic!("expected AutoSuffix, got {:?}", std::mem::discriminant(&other)),
    }
}

#[test]
fn validate_dispatch_skips_dir_warning_for_non_writing_tasks() {
    assert!(validate_dispatch(&RunArgs { prompt: "Research: compare the agent options".to_string(), ..Default::default() }, &AgentKind::Codex).is_empty());
    assert!(validate_dispatch(&RunArgs { prompt: "Implement the dispatcher".to_string(), read_only: true, ..Default::default() }, &AgentKind::Codex).is_empty());
}

#[test]
fn resolve_max_duration_mins_uses_timeout_when_minutes_missing() { assert_eq!(resolve_max_duration_mins(Some(300), None), Some(5)); assert_eq!(resolve_max_duration_mins(Some(301), None), Some(6)); }

#[test]
fn resolve_max_duration_mins_preserves_explicit_minutes() { assert_eq!(resolve_max_duration_mins(Some(300), Some(2)), Some(2)); }

#[test]
fn auto_save_creates_output_for_research_task() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    let log_path = temp.path().join("research.jsonl");
    std::fs::write(&log_path, "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"saved output\"}\n").unwrap();
    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-research-save");
    task.status = TaskStatus::Done;
    task.exit_code = None;
    task.log_path = Some(log_path.display().to_string());
    store.insert_task(&task).unwrap();
    auto_save_task_output(&store, &task).unwrap();
    let output_path = crate::paths::task_dir(task.id.as_str()).join("output.md");
    assert_eq!(std::fs::read_to_string(&output_path).unwrap(), "saved output");
    assert_eq!(store.get_task(task.id.as_str()).unwrap().unwrap().output_path, Some(output_path.display().to_string()));
}

#[tokio::test]
async fn dry_run_returns_without_starting_task() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = run(
        store.clone(),
        RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Inspect the repository state".to_string(),
            dry_run: true,
            skills: vec![NO_SKILL_SENTINEL.to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
    assert!(task.resolved_prompt.is_some());
    assert!(task.prompt_tokens.is_some());
}

#[tokio::test]
async fn run_records_worktree_setup_failure_event() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let task_id = TaskId("t-worktree-fail".to_string());

    let err = run(
        store.clone(),
        RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Inspect the repository state".to_string(),
            dir: Some(temp.path().display().to_string()),
            worktree: Some("aid-worktree-fail".to_string()),
            dry_run: true,
            skills: vec![NO_SKILL_SENTINEL.to_string()],
            existing_task_id: Some(task_id.clone()),
            ..Default::default()
        },
    )
    .await
    .unwrap_err();

    assert!(err.to_string().contains("Not a git repository"));
    assert_eq!(
        store.get_task(task_id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    let events = store.get_events(task_id.as_str()).unwrap();
    assert!(events.iter().any(|event| {
        event.detail.contains("Failed during worktree setup: Not a git repository")
    }));
}

#[tokio::test]
async fn rate_limited_agent_without_cascade_fails_early() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    crate::rate_limit::mark_rate_limited(&AgentKind::Kilo, "try again at Mar 21st, 2099 2:27 PM.");
    let err = run(Arc::new(Store::open_memory().unwrap()), RunArgs {
        agent_name: "kilo".to_string(),
        prompt: "Inspect the repository state".to_string(),
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap_err();
    assert!(err.to_string().contains("kilo is rate-limited until Mar 21st, 2099 2:27 PM"));
}

#[tokio::test]
async fn rate_limited_agent_with_cascade_proceeds() {
    let temp = TempDir::new().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    crate::rate_limit::mark_rate_limited(&AgentKind::Kilo, "try again at Mar 21st, 2099 2:27 PM.");
    let task_id = run(store.clone(), RunArgs {
        agent_name: "kilo".to_string(),
        prompt: "Inspect the repository state".to_string(),
        cascade: vec!["codex".to_string()],
        dry_run: true,
        skills: vec![NO_SKILL_SENTINEL.to_string()],
        ..Default::default()
    }).await.unwrap();
    let task = store.get_task(task_id.as_str()).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Pending);
}

fn make_failed_task(task_id: &str) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
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
        start_sha: None,
        log_path: None,
        output_path: None,
        tokens: None,
        prompt_tokens: None,
        duration_ms: None,
        model: None,
        cost_usd: None,
        exit_code: Some(1),
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: VerifyStatus::Failed,
        pending_reason: None,
        read_only: false,
        budget: false,
    }
}

#[path = "run_transcript_tests.rs"]
mod run_transcript_tests;

#[path = "run_async_tests.rs"]
mod run_async_tests;
