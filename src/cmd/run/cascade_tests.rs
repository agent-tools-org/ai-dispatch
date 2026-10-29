// Tests for cascade retry argument inheritance.
// Covers worktree/dir reuse when a fallback agent takes over a failed task.
// Deps: run_lifecycle helper, RunArgs, task domain types.

use super::{run_lifecycle::inherit_cascade_target, RunArgs};
use crate::support::test_subprocess;
use crate::types::{Task, TaskId, TaskStatus, VerifyStatus};




use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("file.txt", "hello\n", "init")
}

fn failed_task(path: &str) -> Task {
    Task {
        worktree_path: Some(path.to_string()), worktree_branch: Some("feat/cascade".to_string()),
        ..crate::types::Task::test_fixture(TaskId("t-cascade".to_string()), TaskStatus::Failed)
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
        agent_name: "grok".to_string(),
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
        agent_name: "grok".to_string(),
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
fn terminal_failure_preserves_failed_status_for_cascade() {
    let repo = init_repo();
    let linked_root = tempfile::tempdir().unwrap();
    let linked = linked_root.path().join("linked");
    git(repo.path(), &["worktree", "add", "-b", "feat/cascade-refuse", &linked.to_string_lossy()]);

    let _home_dir = crate::support::test_env::TestHome::new();
    std::fs::create_dir_all(crate::support::paths::logs_dir()).unwrap();
    std::fs::write(
        crate::support::paths::stderr_path("t-empty-cascade"),
        "Error: Quota limit reached.",
    )
    .unwrap();

    let store = crate::store::Store::open_memory().unwrap();
    let mut task = failed_task(&linked.display().to_string());
    task.id = TaskId("t-empty-cascade".to_string());
    task.verify_status = VerifyStatus::Passed;
    store.insert_task(&task).unwrap();

    assert!(!store.update_task_status(task.id.as_str(), TaskStatus::Done).unwrap());
    assert!(store.get_events(task.id.as_str()).unwrap().is_empty());

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
