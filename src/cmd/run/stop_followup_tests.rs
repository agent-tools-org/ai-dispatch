// Parent-stop regressions for automatic continuations and serialized launch.
// Deps: run handoffs, real Store mutations, and a deterministic judge test response.

use super::*;
use std::cell::RefCell;
use std::io::Write;
use std::os::unix::net::UnixListener;

#[path = "verify_gate_fixture.rs"]
mod fixture;

type JudgeHook = Box<dyn FnOnce() -> Result<judge::JudgeResult>>;
thread_local! {
    static JUDGE_HOOK: RefCell<Option<JudgeHook>> = RefCell::new(None);
}

pub(crate) fn judge_result() -> Option<Result<judge::JudgeResult>> {
    JUDGE_HOOK
        .with(|hook| hook.borrow_mut().take())
        .map(|hook| hook())
}

fn setup() -> (
    tempfile::TempDir,
    paths::AidHomeGuard,
    Arc<Store>,
    Task,
    RunArgs,
) {
    let home = tempfile::tempdir().unwrap();
    let _permit = crate::test_subprocess::acquire();
    assert!(
        std::process::Command::new("git")
            .args(["init", "-b", "main"])
            .current_dir(home.path())
            .output()
            .unwrap()
            .status
            .success()
    );
    let guard = paths::AidHomeGuard::set(home.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let dir = home.path().to_str().unwrap();
    let mut parent = fixture::task("t-parent", TaskStatus::Done, None, None);
    parent.repo_path = Some(dir.to_string());
    parent.worktree_branch = None;
    store.insert_task(&parent).unwrap();
    let args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Investigate task continuation".to_string(),
        dir: Some(dir.to_string()),
        dry_run: true,
        ..Default::default()
    };
    store
        .update_task_dispatch_args(parent.id.as_str(), &args.dispatch_args_json().unwrap())
        .unwrap();
    (home, guard, store, parent, args)
}

fn assert_refused(store: &Store, result: Result<Option<TaskId>>) {
    assert_eq!(
        store.list_tasks(TaskFilter::All).unwrap().len(),
        1,
        "no child row"
    );
    assert_eq!(
        store.get_task("t-parent").unwrap().unwrap().status,
        TaskStatus::Stopped
    );
    let error = result.expect_err("stopped parent must refuse child dispatch");
    assert!(error.to_string().contains("t-parent"), "{error}");
    assert!(error.to_string().contains("stopped"), "{error}");
    assert_eq!(
        std::fs::read_dir(paths::jobs_dir()).unwrap().count(),
        0,
        "no worker spec"
    );
}

#[tokio::test]
async fn stop_followup_verify_failure_after_parent_stop_creates_nothing() {
    let (_home, _guard, store, parent, mut args) = setup();
    args.verify = Some("false".to_string());
    args.retry = 1;
    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());
    maybe_verify(&store, &parent.id, Some("false"), args.dir.as_deref(), None);
    assert_eq!(
        store
            .get_task(parent.id.as_str())
            .unwrap()
            .unwrap()
            .verify_status,
        VerifyStatus::Failed
    );

    let result =
        maybe_auto_retry_after_verify_failure(&store, &parent.id, &args, TaskStatus::Done).await;

    assert_refused(&store, result);
}

#[tokio::test]
async fn stop_followup_judge_retry_after_parent_stop_creates_nothing() {
    let (_home, _guard, store, parent, mut args) = setup();
    args.judge = Some("codex".to_string());
    let stop_store = store.clone();
    JUDGE_HOOK.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            assert!(crate::task_lifecycle::mark_stopped(
                &stop_store,
                "t-parent"
            )?);
            Ok(judge::JudgeResult {
                passed: false,
                feedback: "add tests".to_string(),
            })
        }));
    });

    let result = maybe_judge_retry(&store, &args, &parent.id).await;

    assert_refused(&store, result);
}

#[tokio::test]
async fn stop_followup_iteration_after_parent_stop_creates_nothing() {
    let (home, _guard, store, parent, args) = setup();
    let _permit = crate::test_subprocess::acquire();
    let socket = home.path().join("eval.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let stop_store = store.clone();
    let stopper = std::thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        assert!(crate::task_lifecycle::mark_stopped(&stop_store, "t-parent").unwrap());
        stream.write_all(b"x").unwrap();
    });
    let config = IterateConfig {
        max_iterations: 2,
        eval_command: format!(
            concat!(
                "python3 -c 'import socket; s=socket.socket(socket.AF_UNIX); ",
                "s.connect(\"{}\"); s.recv(1); exit(1)'"
            ),
            socket.display()
        ),
        feedback_template: None,
    };

    let result = maybe_iterate(&store, &parent.id, &args, &config).await;
    stopper.join().unwrap();

    assert_refused(&store, result);
}

#[test]
fn stop_followup_child_insert_after_stop_is_refused() {
    let (_home, _guard, store, parent, _) = setup();
    let mut child = fixture::task("t-child", TaskStatus::Pending, None, None);
    child.parent_task_id = Some(parent.id.to_string());
    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());

    let error = store
        .insert_task(&child)
        .expect_err("child insert must refuse stopped parent");

    assert!(error.to_string().contains("t-parent"));
    assert!(store.get_task(child.id.as_str()).unwrap().is_none());
}

#[test]
fn stop_followup_waiting_replacement_after_stop_is_refused() {
    let (_home, _guard, store, parent, _) = setup();
    let mut child = fixture::task("t-child", TaskStatus::Waiting, None, None);
    store.insert_task(&child).unwrap();
    child.status = TaskStatus::Pending;
    child.parent_task_id = Some(parent.id.to_string());
    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());

    assert!(store.replace_waiting_task(&child).is_err());

    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Waiting
    );
}

#[test]
fn stop_followup_parent_stop_between_insert_and_launch_prevents_worker() {
    let (_home, _guard, store, parent, _) = setup();
    let mut child = fixture::task("t-child", TaskStatus::Pending, None, None);
    child.parent_task_id = Some(parent.id.to_string());
    store.insert_task(&child).unwrap();
    assert!(crate::task_lifecycle::mark_running(&store, &child.id).unwrap());
    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());
    let mut launched = false;

    let result = store.with_worker_launch(child.id.as_str(), || {
        launched = true;
        Ok(())
    });

    assert!(!launched, "launch closure must not run");
    let error = result.unwrap_err();
    assert!(error.to_string().contains("t-parent"));
    let saved = store.get_task(child.id.as_str()).unwrap().unwrap();
    assert_eq!(saved.status, TaskStatus::Stopped);
    assert!(saved.completed_at.is_some());
    assert!(saved.duration_ms.is_some());
    assert!(
        store
            .get_events(child.id.as_str())
            .unwrap()
            .iter()
            .any(|event| { event.detail.contains("t-parent") && event.detail.contains("stopped") })
    );
}

#[test]
fn stop_followup_background_dispatch_after_parent_stop_writes_no_spec() {
    let (_home, _guard, store, parent, mut args) = setup();
    args.parent_task_id = Some(parent.id.to_string());
    let prepared =
        run_dispatch_prepare::prepare_dispatch_with(&store, &mut args, |_| true).unwrap();
    assert!(crate::task_lifecycle::mark_running(&store, &prepared.task_id).unwrap());
    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());
    let prompt = PromptBundle {
        effective_prompt: args.prompt.clone(),
        context_files: Vec::new(),
        prompt_tokens: 0,
        injected_memory_ids: Vec::new(),
    };

    let error = run_dispatch_execute::run_background_task(&store, &args, &prepared, &prompt)
        .expect_err("no worker may start after the parent stop");

    assert!(error.to_string().contains("t-parent"));
    assert!(!paths::job_path(prepared.task_id.as_str()).exists());
    assert_eq!(
        store
            .get_task(prepared.task_id.as_str())
            .unwrap()
            .unwrap()
            .status,
        TaskStatus::Stopped
    );
}

#[test]
fn stop_followup_child_launched_before_parent_stop_is_unaffected() {
    let (_home, _guard, store, parent, _) = setup();
    let mut child = fixture::task("t-child", TaskStatus::Running, None, None);
    child.parent_task_id = Some(parent.id.to_string());
    store.insert_task(&child).unwrap();
    let mut launched = false;
    store
        .with_worker_launch(child.id.as_str(), || {
            launched = true;
            Ok(())
        })
        .unwrap();

    assert!(crate::task_lifecycle::mark_stopped(&store, parent.id.as_str()).unwrap());

    assert!(launched);
    store
        .with_running_task(child.id.as_str(), || Ok(()))
        .unwrap();
    assert_eq!(
        store.get_task(child.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Running
    );
    assert!(crate::task_lifecycle::mark_stopped(&store, child.id.as_str()).unwrap());
}
