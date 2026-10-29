// Regression tests for background worker error lifecycle convergence.
// Covers post-run fallback effects after run_task_inner returns an operational error.
// Deps: background error handler, Store, local webhook listener, task fixtures.

use super::{BackgroundRunSpec, handle_run_task_inner_error};
use crate::cmd::run::RunArgs;
use crate::support::paths;
use crate::types::{AgentKind, Task, TaskId, TaskStatus};

use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn worker_keeps_spec_until_error_lifecycle_finishes() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = "t-bg-barrier";
    // The agent resolves for the lifecycle but its binary is missing, so the
    // worker fails before launching anything.
    std::fs::create_dir_all(paths::aid_dir().join("agents")).unwrap();
    std::fs::write(paths::aid_dir().join("agents/missing-bin-test.toml"),
        "[agent]\nid = \"missing-bin-test\"\ndisplay_name = \"Missing Bin\"\ncommand = \"/nonexistent/aid-test-agent\"\nprompt_mode = \"arg\"\ninteractive_input = false\n").unwrap();
    let mut row = task(id, TaskStatus::Running);
    row.agent = AgentKind::Custom;
    row.custom_agent_name = Some("missing-bin-test".into());
    store.insert_task(&row).unwrap();
    let observed = temp.path().join("barrier.txt");
    let args = RunArgs {
        agent_name: "missing-bin-test".into(),
        hooks: vec![format!("on_fail:test -f '{}' && printf present > '{}'",
            paths::job_path(id).display(), observed.display())],
        ..Default::default()
    };
    store.update_task_dispatch_args(id, &args.dispatch_args_json().unwrap()).unwrap();
    super::save_spec(&spec(id)).unwrap();
    assert!(super::run_task(store.clone(), id).await.is_err());
    assert_eq!(std::fs::read_to_string(observed).unwrap(), "present");
    assert!(super::load_spec_if_exists(id).unwrap().is_none());
    assert_eq!(store.get_task(id).unwrap().unwrap().status, TaskStatus::Failed);
}

#[tokio::test]
async fn inner_error_runs_post_lifecycle_once_without_duplicate_callbacks() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    store
        .insert_task(&task("t-bg-error", TaskStatus::Running))
        .unwrap();
    let (webhook_url, webhook_count) = start_webhook_counter();
    write_webhook_config(&webhook_url);
    let hook_path = temp.path().join("hook.txt");
    let done_path = temp.path().join("done.txt");
    let args = RunArgs {
        agent_name: "codex".into(),
        hooks: vec![format!("on_fail:printf fail >> '{}'", hook_path.display())],
        ..Default::default()
    };
    let mut spec = spec("t-bg-error");
    spec.on_done = Some(format!("printf done >> '{}'", done_path.display()));

    let _ = handle_run_task_inner_error(&store, &spec, Some(&args), anyhow::anyhow!("setup failed")).await;
    let _ = handle_run_task_inner_error(&store, &spec, Some(&args), anyhow::anyhow!("late duplicate")).await;

    assert_eq!(read_wait(&hook_path), "fail");
    assert_eq!(read_wait(&done_path), "done");
    assert_eq!(webhook_count.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
}

#[tokio::test]
async fn inner_error_appends_completion_exactly_once() {
    let (_temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    store
        .insert_task(&task("t-bg-completion", TaskStatus::Running))
        .unwrap();
    let spec = spec("t-bg-completion");
    let args = RunArgs { agent_name: "codex".into(), ..Default::default() };

    let _ = handle_run_task_inner_error(&store, &spec, Some(&args), anyhow::anyhow!("boom")).await;

    let completions_path = paths::aid_dir().join("completions.jsonl");
    let content = std::fs::read_to_string(&completions_path).unwrap_or_default();
    let count = content.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(
        count, 1,
        "expected exactly one completions.jsonl entry, got: {content}"
    );
}

#[tokio::test]
async fn worker_without_saved_args_fails_the_task_and_notifies_once() {
    let (temp, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let id = "t-bg-unsaved";
    store.insert_task(&task(id, TaskStatus::Running)).unwrap();
    let done_path = temp.path().join("done.txt");
    let mut job = spec(id);
    job.on_done = Some(format!("printf \"$AID_TASK_STATUS\" > '{}'", done_path.display()));
    super::save_spec(&job).unwrap();

    let err = super::run_task(store.clone(), id).await.unwrap_err();

    assert!(format!("{err:#}").contains("no saved dispatch args"), "{err:#}");
    assert_eq!(store.get_task(id).unwrap().unwrap().status, TaskStatus::Failed);
    assert!(super::load_spec_if_exists(id).unwrap().is_none());
    let completions = std::fs::read_to_string(paths::aid_dir().join("completions.jsonl")).unwrap();
    assert_eq!(completions.lines().filter(|line| line.contains(id)).count(), 1);
    assert_eq!(read_wait(&done_path), "failed", "the job's on_done still reports the failure");
}

fn start_webhook_counter() -> (String, mpsc::Receiver<usize>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        listener.set_nonblocking(true).unwrap();
        let deadline = Instant::now() + Duration::from_secs(2);
        let mut count = 0;
        while Instant::now() < deadline {
            match listener.accept() {
                Ok((mut stream, _)) => {
                    count += 1;
                    let _ = stream.set_read_timeout(Some(Duration::from_millis(100)));
                    let mut buffer = [0; 1024];
                    let _ = stream.read(&mut buffer);
                    let _ = stream.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 0\r\n\r\n");
                }
                Err(err) if err.kind() == std::io::ErrorKind::WouldBlock => {
                    std::thread::sleep(Duration::from_millis(20));
                }
                Err(_) => break,
            }
        }
        let _ = tx.send(count);
    });
    (format!("http://{addr}/hook"), rx)
}

fn write_webhook_config(url: &str) {
    let content = format!(
        "[[webhook]]\nname = \"test\"\nurl = \"{url}\"\non_failed = true\n"
    );
    std::fs::write(paths::config_path(), content).unwrap();
}

fn read_wait(path: &std::path::Path) -> String {
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        if let Ok(content) = std::fs::read_to_string(path) {
            if !content.is_empty() {
                return content;
            }
        }
        if Instant::now() >= deadline {
            return std::fs::read_to_string(path).unwrap_or_default();
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

fn spec(task_id: &str) -> BackgroundRunSpec {
    BackgroundRunSpec {
        task_id: task_id.to_string(),
        worker_pid: Some(123),
        interactive: true,
        ..Default::default()
    }
}

fn task(task_id: &str, status: TaskStatus) -> Task {
    crate::types::Task::test_fixture(TaskId(task_id.to_string()), status)
}
