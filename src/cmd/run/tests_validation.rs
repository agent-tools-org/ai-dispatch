// Extracted validation tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn rescue_quota_failed_task_rescues_committed_work_non_standard_branch() {
    let dir = TempDir::new().unwrap();
    let _guard = paths::AidHomeGuard::set(dir.path());
    std::fs::create_dir_all(paths::logs_dir()).unwrap();

    let wt_dir = dir.path().join("wt");
    std::fs::create_dir_all(&wt_dir).unwrap();
    git(&wt_dir, &["init", "-b", "feature-custom"]);
    git(&wt_dir, &["config", "user.email", "aid@example.com"]);
    git(&wt_dir, &["config", "user.name", "Aid Tester"]);
    std::fs::write(wt_dir.join("file.txt"), "initial").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "initial"]);

    let start_sha = crate::commit::head_sha(wt_dir.to_str().unwrap()).unwrap();

    std::fs::write(
        paths::stderr_path("t-custom-branch"),
        "Error: You have hit your usage limit.",
    )
    .unwrap();

    let store = Store::open_memory().unwrap();
    let mut task = make_failed_task("t-custom-branch");
    task.worktree_path = Some(wt_dir.to_str().unwrap().to_string());
    task.start_sha = Some(start_sha);
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    // Untouched: guard refuses rescue even on non-standard branch with base_branch=None.
    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );
    let checked = store.get_task("t-custom-branch").unwrap().unwrap();
    assert_eq!(checked.status, TaskStatus::Failed);

    // Agent commits work on feature-custom branch.
    std::fs::write(wt_dir.join("file.txt"), "agent commit").unwrap();
    git(&wt_dir, &["add", "file.txt"]);
    git(&wt_dir, &["commit", "-m", "agent commit"]);

    rescue_quota_failed_task(
        &store,
        &task.id,
        read_quota_error_message(&task.id, &task.agent).as_deref(),
    );
    let rescued = store.get_task("t-custom-branch").unwrap().unwrap();
    assert_eq!(rescued.status, TaskStatus::Done);
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
        read_quota_error_message(&task.id, &task.agent).as_deref(),
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
        read_quota_error_message(&task.id, &task.agent).as_deref(),
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
fn validate_dispatch_warns_copilot_without_dir() {
    assert_eq!(validate_dispatch(&RunArgs { prompt: "Implement the dispatcher".to_string(), ..Default::default() }, &AgentKind::Copilot), vec!["Code agent without --dir may not be able to write files".to_string()]);
}

#[test]
fn validate_dispatch_stays_silent_when_worktree_supplies_the_dir() {
    let args = RunArgs {
        prompt: "Implement the dispatcher".to_string(),
        worktree: Some("fix/some-branch".to_string()),
        ..Default::default()
    };
    assert!(validate_dispatch(&args, &AgentKind::Codex).is_empty());
    assert!(validate_dispatch(&args, &AgentKind::Cursor).is_empty());
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
        None,
        None,
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
        None,
        None,
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
