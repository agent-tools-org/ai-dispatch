// Shared synthetic dispatch setup for app-server integration tests.
// Uses a fake Codex executable and a real task store without external credentials.

use crate::{store::Store, support::paths, types::*};
use std::path::PathBuf;
use std::process::Command;
use std::sync::Arc;

pub(super) struct Fixture {
    pub home: tempfile::TempDir,
    pub _guard: paths::AidHomeGuard,
    pub store: Arc<Store>,
    pub task: TaskId,
    pub command: Command,
    pub log: PathBuf,
    pub output: PathBuf,
}

impl Fixture {
    pub fn new(mode: &str) -> Self {
        let home = tempfile::tempdir().unwrap();
        let guard = paths::AidHomeGuard::set(home.path());
        paths::ensure_dirs().unwrap();
        let store = Arc::new(Store::open_memory().unwrap());
        let task = TaskId("t-app-server".into());
        insert_task(&store, &task);
        let command = command(home.path(), mode);
        let log = paths::log_path(task.as_str());
        let output = home.path().join("output.txt");
        Self {
            home,
            _guard: guard,
            store,
            task,
            command,
            log,
            output,
        }
    }

    pub fn dispatch(&self) {
        let mut policy = crate::config::timeout_policy::TimeoutPolicy::default();
        policy.idle = std::time::Duration::from_secs(2);
        self.dispatch_with_policy(policy);
    }

    pub fn dispatch_with_policy(&self, policy: crate::config::timeout_policy::TimeoutPolicy) {
        let _permit = crate::support::test_subprocess::acquire();
        let task = self.store.get_task(self.task.as_str()).unwrap().unwrap();
        let adapter = crate::agent::codex::routed(task.custom_agent_name);
        crate::runtime::pty::runner::run_agent_process_with_control(
            adapter.as_ref(),
            &self.command,
            &self.task,
            &self.store,
            &self.log,
            Some(self.output.to_str().unwrap()),
            None,
            true,
            policy,
            None,
            None,
            std::time::Instant::now(),
        )
        .unwrap();
    }

    pub fn actor(&self, stop: bool) -> std::thread::JoinHandle<()> {
        self.actor_mode(if stop { "stop" } else { "reply" })
    }

    pub fn actor_mode(&self, mode: &'static str) -> std::thread::JoinHandle<()> {
        let store = self.store.clone();
        let task = self.task.clone();
        let home = self.home.path().to_path_buf();
        std::thread::spawn(move || {
            let _guard = paths::AidHomeGuard::set(&home);
            let deadline = std::time::Instant::now() + std::time::Duration::from_secs(10);
            while !crate::runtime::codex_app_server::active(task.as_str()) {
                assert!(
                    std::time::Instant::now() < deadline,
                    "app-server did not become active"
                );
                std::thread::park_timeout(std::time::Duration::from_millis(10));
            }
            act(&store, &task, mode);
        })
    }
}

fn insert_task(store: &Store, task: &TaskId) {
    store
        .insert_waiting_task(
            task.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .db()
        .execute(
            "UPDATE tasks SET status='running' WHERE id=?1",
            [task.as_str()],
        )
        .unwrap();
}

fn command(home: &std::path::Path, mode: &str) -> Command {
    use std::os::unix::fs::PermissionsExt;
    let real = home.join("real-codex");
    std::fs::create_dir(&real).unwrap();
    std::fs::create_dir(real.join("sessions")).unwrap();
    std::fs::write(real.join("state_5.sqlite"), "synthetic state").unwrap();
    std::fs::write(
        real.join("config.toml"),
        "sandbox_mode='danger-full-access'",
    )
    .unwrap();
    std::fs::write(real.join("AGENTS.md"), "synthetic instructions").unwrap();
    let executable = home.join("codex");
    std::fs::write(
        &executable,
        include_str!("../../../../tests/fixtures/codex-app-server.py"),
    )
    .unwrap();
    std::fs::set_permissions(&executable, std::fs::Permissions::from_mode(0o700)).unwrap();
    let fixture = home.join("fixture.jsonl");
    std::fs::write(
        &fixture,
        include_str!("../../../../tests/fixtures/codex-app-server-steer.jsonl"),
    )
    .unwrap();
    let mut command = Command::new(executable);
    command.args([
        "exec",
        "--json",
        "literal\n{\"method\":\"turn/interrupt\"}",
        "--ignore-user-config",
    ]);
    command.arg("-o").arg(home.join("output.txt"));
    command
        .env("CODEX_HOME", real)
        .env("AID_FAKE_FIXTURE", fixture)
        .env("AID_FAKE_MODE", mode)
        .env("AID_FAKE_INTERRUPTED", home.join("interrupted"))
        .env("AID_FAKE_FLUSHED", home.join("flushed"));
    command
}

fn act(store: &Arc<Store>, task: &TaskId, mode: &str) {
    if mode == "stop" || mode == "timed-stop" {
        let start = std::time::Instant::now();
        crate::cmd::stop::stop(store, task.as_str()).unwrap();
        if mode == "timed-stop" {
            assert!(start.elapsed() >= std::time::Duration::from_millis(2400));
        }
    } else if mode == "steer" {
        crate::cmd::steer::run(store, task.as_str(), "steered direction").unwrap();
    } else {
        let outcome = crate::cmd::reply::run(
            store,
            task.as_str(),
            Some("new direction"),
            None,
            mode == "reply",
            2,
        )
        .unwrap();
        if mode == "reply" {
            assert!(matches!(
                outcome,
                crate::cmd::reply::ReplyOutcome::Queued { .. }
            ));
        } else {
            assert!(matches!(
                outcome,
                crate::cmd::reply::ReplyOutcome::Accepted { .. }
            ));
        }
        let task = store.get_task(task.as_str()).unwrap().unwrap();
        assert!(
            crate::cmd::reply::ensure_interactive_input(
                &task,
                crate::cmd::reply::InputCommand::Respond
            )
            .is_err()
        );
    }
}
