// Tests for cascade retry argument inheritance.
// Covers worktree/dir reuse when a fallback agent takes over a failed task.
// Deps: run_lifecycle helper, RunArgs, task domain types.

use super::{run_lifecycle::inherit_cascade_target, RunArgs};
use crate::test_subprocess;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::path::Path;
use std::process::Command;

fn git(repo_dir: &Path, args: &[&str]) {
    assert!(Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(args)
        .status()
        .unwrap()
        .success());
}

fn init_repo() -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("file.txt"), "hello\n").unwrap();
    git(repo.path(), &["add", "file.txt"]);
    git(repo.path(), &["commit", "-m", "init"]);
    repo
}

fn failed_task(path: &str) -> Task {
    Task {
        id: TaskId("t-cascade".to_string()), agent: AgentKind::Codex, custom_agent_name: None,
        prompt: "prompt".to_string(), resolved_prompt: None, category: None,
        status: TaskStatus::Failed, parent_task_id: None, workgroup_id: None,
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None,
        worktree_path: Some(path.to_string()), worktree_branch: Some("feat/cascade".to_string()),
        final_head_sha: None,
        final_branch: None,
        start_sha: None, log_path: None, output_path: None, tokens: None, prompt_tokens: None,
        duration_ms: None, requested_model: None, observed_model: None, attribution_source: None, cost_usd: None, exit_code: None,
        created_at: Local::now(), completed_at: None, verify: None,
        verify_status: VerifyStatus::Skipped, pending_reason: None, read_only: false,
        budget: false, audit_verdict: None, audit_report_path: None, delivery_assessment: None,
    }
}

fn failed_task_with_repo(repo: &str, path: &str) -> Task {
    let mut task = failed_task(path);
    task.repo_path = Some(repo.to_string());
    task
}

#[test]
fn cascade_inherits_existing_worktree_dir() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", "feat/cascade", &linked.to_string_lossy()]);
    let task = failed_task(&linked.display().to_string());
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    inherit_cascade_target(&mut args, &task).unwrap();

    assert_eq!(args.dir, task.worktree_path);
    // The cascade keeps the worktree branch so the follow-up task is persisted with its
    // worktree metadata intact; dropping it strips isolation from later generations.
    assert_eq!(args.worktree.as_deref(), Some("feat/cascade"));
    git(repo.path(), &["worktree", "remove", "--force", &linked.to_string_lossy()]);
}

#[test]
fn cascade_refuses_persisted_worktree_that_equals_repo_path() {
    let temp = tempfile::tempdir().unwrap();
    let repo_path = temp.path().display().to_string();
    let task = failed_task_with_repo(&repo_path, &repo_path);
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    let err = inherit_cascade_target(&mut args, &task).unwrap_err();

    assert!(err.to_string().contains("recorded worktree path"));
}

/// A cascade exists to escape a failing route. Carrying the parent's model
/// across the agent switch sent agy `gpt-5.6-luna` — codex's model — and agy
/// refused it by listing its own (`t-ac9a7a9d`, cascaded from `t-90371f9e`).
#[test]
fn cascading_to_another_agent_drops_the_parents_model() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        model: Some("gpt-5.6-luna".to_string()),
        cascade: vec!["agy".to_string(), "cursor".to_string()],
        ..Default::default()
    };
    let (next, remaining) =
        super::take_next_cascade_agent(&args).expect("cascade must yield the next agent");
    assert_eq!(next, "agy");
    assert_eq!(remaining, vec!["cursor".to_string()]);

    // The guard the dispatch path applies.
    if args.agent_name != next {
        args.model = None;
    }
    args.agent_name = next;
    assert_eq!(args.model, None, "a model chosen for codex must not reach agy");
}

/// The mirror case: a cascade entry naming the same agent is a retry, and a
/// retry must still ask for what was asked before.
#[test]
fn cascading_to_the_same_agent_keeps_the_model() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        model: Some("gpt-5.6-luna".to_string()),
        cascade: vec!["codex".to_string()],
        ..Default::default()
    };
    let (next, _) = super::take_next_cascade_agent(&args).expect("cascade must yield an agent");
    if args.agent_name != next {
        args.model = None;
    }
    assert_eq!(args.model.as_deref(), Some("gpt-5.6-luna"));
}

#[test]
fn refused_quota_rescue_preserves_failed_status_for_cascade() {
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", "feat/cascade-refuse", &linked.to_string_lossy()]);

    let home_dir = tempfile::tempdir().unwrap();
    let _guard = crate::paths::AidHomeGuard::set(home_dir.path());
    std::fs::create_dir_all(crate::paths::logs_dir()).unwrap();
    std::fs::write(
        crate::paths::stderr_path("t-empty-cascade"),
        "Error: Quota limit reached.",
    )
    .unwrap();

    let store = crate::store::Store::open_memory().unwrap();
    let mut task = failed_task(&linked.display().to_string());
    task.id = TaskId("t-empty-cascade".to_string());
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    super::run_post::rescue_quota_failed_task(
        &store,
        &task.id,
        super::run_post::read_quota_error_message(&task.id).as_deref(),
        None,
    );

    let saved_task = store.get_task("t-empty-cascade").unwrap().unwrap();
    assert_eq!(saved_task.status, TaskStatus::Failed);

    let args = RunArgs {
        agent_name: "oz".to_string(),
        cascade: vec!["codebuff".to_string()],
        ..Default::default()
    };
    assert_eq!(saved_task.status, TaskStatus::Failed);
    let next_cascade = super::run_post::take_next_cascade_agent(&args);
    assert_eq!(next_cascade, Some(("codebuff".to_string(), vec![])));

    git(repo.path(), &["worktree", "remove", "--force", &linked.to_string_lossy()]);
}
