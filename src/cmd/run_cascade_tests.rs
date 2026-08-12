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
        caller_kind: None, caller_session_id: None, agent_session_id: None, repo_path: None, project_id: None,
        worktree_path: Some(path.to_string()), effective_dir: None, worktree_branch: Some("feat/cascade".to_string()),
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
    let task = failed_task_with_repo(&repo.path().display().to_string(), &linked.display().to_string());
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        worktree: Some("feat/cascade".to_string()),
        ..Default::default()
    };

    inherit_cascade_target(&mut args, &task).unwrap();

    assert_eq!(args.dir, task.worktree_path);
    assert_eq!(args.repo.as_deref(), Some(repo.path().to_string_lossy().as_ref()));
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

#[test]
fn cascade_without_any_persisted_target_keeps_caller_directory_defaults() {
    let mut task = failed_task("unused");
    task.worktree_path = None;
    task.worktree_branch = None;
    let mut args = RunArgs {
        existing_task_id: Some(crate::types::TaskId("t-parent".to_string())),
        ..Default::default()
    };

    inherit_cascade_target(&mut args, &task).unwrap();

    assert_eq!(args.dir, None);
    assert_eq!(args.repo, None);
    assert_eq!(args.worktree, None);
    assert_eq!(args.existing_task_id, None);
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
        super::run_post::read_quota_error_message(&task.id, &task.agent).as_deref(),
    );

    let saved_task = store.get_task("t-empty-cascade").unwrap().unwrap();
    assert_eq!(saved_task.status, TaskStatus::Failed);

    let args = RunArgs {
        agent_name: "oz".to_string(),
        cascade: vec!["kilo".to_string()],
        ..Default::default()
    };
    assert_eq!(saved_task.status, TaskStatus::Failed);
    let next_cascade = super::run_post::take_next_cascade_agent(&args);
    assert_eq!(next_cascade, Some(("kilo".to_string(), vec![])));

    git(repo.path(), &["worktree", "remove", "--force", &linked.to_string_lossy()]);
}

/// A model id means something only inside one CLI. codex's `gpt-5.6-sol`
/// reaching agy is refused outright, and agy answers by listing its Gemini
/// models — t-94d5f8ab, auto-cascaded from t-9269aab8 after codex hit quota.
///
/// v10.5.1 claimed to drop the model in every switch path and dropped it in
/// three, missing four — including both cascades in run_lifecycle, which is the
/// path a quota failure actually takes. The rule now lives in one function; this
/// pins it there so a sixth call site cannot quietly reintroduce the bug.
#[test]
fn switching_agent_drops_a_model_that_belongs_to_the_old_cli() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        model: Some("gpt-5.6-sol".to_string()),
        ..Default::default()
    };
    super::run_post::switch_agent(&mut args, "agy".to_string());
    assert_eq!(args.agent_name, "agy");
    assert_eq!(args.model, None, "a codex model must not survive into agy");
}

/// A same-agent retry is not a route change and must still ask for what was
/// asked before.
#[test]
fn retrying_the_same_agent_keeps_its_model() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        model: Some("gpt-5.6-sol".to_string()),
        ..Default::default()
    };
    super::run_post::switch_agent(&mut args, "codex".to_string());
    assert_eq!(args.model.as_deref(), Some("gpt-5.6-sol"));
}

/// A session id resumes state inside the CLI that issued it. Carrying it across
/// an agent switch is the same defect as carrying a model — one field over.
#[test]
fn switching_agent_drops_a_session_that_belongs_to_the_old_cli() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        session_id: Some("codex-session-abc".to_string()),
        ..Default::default()
    };
    super::run_post::switch_agent(&mut args, "agy".to_string());
    assert_eq!(args.agent_name, "agy");
    assert_eq!(args.session_id, None, "a codex session must not survive into agy");
}

/// A same-agent retry may still resume; switch_agent must not clear the session
/// when the agent name is unchanged.
#[test]
fn retrying_the_same_agent_keeps_its_session() {
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        session_id: Some("codex-session-abc".to_string()),
        ..Default::default()
    };
    super::run_post::switch_agent(&mut args, "codex".to_string());
    assert_eq!(args.session_id.as_deref(), Some("codex-session-abc"));
}
