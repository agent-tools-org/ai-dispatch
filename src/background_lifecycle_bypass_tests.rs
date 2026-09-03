// Regression tests for background worker error lifecycle convergence.
// Covers post-run fallback effects after run_task_inner returns an operational error.
// Deps: background error handler, Store, local webhook listener, task fixtures.

use super::{handle_run_task_inner_error, BackgroundRunSpec};
use crate::paths;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::io::{Read, Write};
use std::net::TcpListener;
use std::sync::mpsc;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn inner_error_runs_post_lifecycle_once_without_duplicate_callbacks() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&task("t-bg-error", TaskStatus::Running))
        .unwrap();
    let (webhook_url, webhook_count) = start_webhook_counter();
    write_webhook_config(&webhook_url);
    let hook_path = temp.path().join("hook.txt");
    let done_path = temp.path().join("done.txt");
    let spec = BackgroundRunSpec {
        hooks: vec![format!("on_fail:printf fail >> '{}'", hook_path.display())],
        on_done: Some(format!("printf done >> '{}'", done_path.display())),
        ..spec("t-bg-error")
    };

    let _ = handle_run_task_inner_error(&store, &spec, anyhow::anyhow!("setup failed")).await;
    let _ = handle_run_task_inner_error(&store, &spec, anyhow::anyhow!("late duplicate")).await;

    assert_eq!(read_wait(&hook_path), "fail");
    assert_eq!(read_wait(&done_path), "done");
    assert_eq!(webhook_count.recv_timeout(Duration::from_secs(3)).unwrap(), 1);
}

#[tokio::test]
async fn inner_error_appends_completion_exactly_once() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    paths::ensure_dirs().unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    store
        .insert_task(&task("t-bg-completion", TaskStatus::Running))
        .unwrap();
    let spec = spec("t-bg-completion");

    let _ = handle_run_task_inner_error(&store, &spec, anyhow::anyhow!("boom")).await;

    let completions_path = paths::aid_dir().join("completions.jsonl");
    let content = std::fs::read_to_string(&completions_path).unwrap_or_default();
    let count = content.lines().filter(|l| !l.is_empty()).count();
    assert_eq!(
        count, 1,
        "expected exactly one completions.jsonl entry, got: {content}"
    );
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
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        session_id: None,
        verify: None,
        setup: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        judge: None,
        max_duration_mins: None,
        max_task_cost: None,
        idle_timeout_secs: None,
        retry: 0,
        group: None,
        skills: vec![],
        checklist: vec![],
        hooks: vec![],
        template: None,
        worktree: None,
        base_branch: None,
        peer_review: None,
        audit: false,
        audit_explicit: false,
        no_audit: false,
        scope: vec![],
        interactive: true,
        on_done: None,
        cascade: vec![],
        parent_task_id: None,
        env: None,
        env_forward: None,
        agent_pid: None,
        sandbox: false,
        read_only: false,
        audit_report_mode: false,
        container: None,
        link_deps: true,
        pre_task_dirty_paths: None,
    }
}

fn task(task_id: &str, status: TaskStatus) -> Task {
    Task {
        id: TaskId(task_id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: None,
        worktree_path: None, effective_dir: None,
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
