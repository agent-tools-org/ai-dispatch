// Tests for final branch capture during `aid stop`.
// Covers stopped tasks preserving readable worktree branch state.
// Deps: stop command, Store, git CLI, worktree creation.

use super::{kill, stop};
use crate::store::Store;
use crate::support::test_subprocess;
use crate::types::{Task, TaskId, TaskStatus};

use std::path::Path;
use std::process::Command;
use std::sync::Arc;

use crate::support::test_env::git;

fn init_repo() -> tempfile::TempDir {
    crate::support::test_repo::repo_with_file("base.txt", "base\n", "base")
}

fn running_task(id: &str, repo: &Path, wt: &Path, branch: &str) -> Task {
    Task {
        repo_path: Some(repo.display().to_string()),
        worktree_path: Some(wt.display().to_string()),
        worktree_branch: Some(branch.to_string()),
        ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Running)
    }
}

#[test]
fn stop_captures_final_branch_before_marking_stopped() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let branch = "fix/stop-final-capture";
    let info = crate::workspace::worktree::create_worktree(repo.path(), branch, None).unwrap();
    git(&info.path, &["switch", "-c", "agent/stop-final"]);
    let store = Arc::new(Store::open_memory().unwrap());
    let task = running_task("t-stop-final-capture", repo.path(), &info.path, branch);
    store.insert_task(&task).unwrap();

    stop(&store, task.id.as_str()).unwrap();

    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.status, TaskStatus::Stopped);
    assert_eq!(loaded.final_branch.as_deref(), Some("agent/stop-final"));
    assert!(loaded.final_head_sha.is_some());
    assert!(info.path.exists());
}

#[test]
fn stop_and_kill_release_worktree_locks() {
    let _permit = test_subprocess::acquire();
    let repo = init_repo();
    let store = Arc::new(Store::open_memory().unwrap());

    for (task_id, branch, status, action) in [
        ("t-stop-lock", "fix/stop-lock", TaskStatus::Running, stop as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
        ("t-kill-lock", "fix/kill-lock", TaskStatus::Running, kill as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
        ("t-pending-lock", "fix/pending-lock", TaskStatus::Pending, stop as fn(&Arc<Store>, &str) -> anyhow::Result<()>),
    ] {
        let info = crate::workspace::worktree::create_worktree(repo.path(), branch, None).unwrap();
        let mut task = running_task(task_id, repo.path(), &info.path, branch);
        task.status = status;
        store.insert_task(&task).unwrap();
        crate::workspace::worktree::try_acquire_worktree_lock(&info.path, task_id).unwrap();

        action(&store, task_id).unwrap();

        assert!(!info.path.join(".aid-lock").exists());
        assert!(crate::workspace::worktree::check_worktree_lock(&info.path).is_none());
    }
}

#[cfg(unix)]
#[test]
fn detached_stop_and_force_kill_reject_unverified_agent_identity() {
    let _permit = test_subprocess::acquire();
    for action in [stop, kill] {
        for started in [Some("previous-launch".to_string()), None] {
            let home = crate::support::test_env::TestHome::new();
            let store = Arc::new(Store::open_memory().expect("store"));
            let mut task = running_task("t-stop-identity", home.path(), home.path(), "unused");
            task.worktree_path = None;
            task.read_only = true;
            store.insert_task(&task).expect("task");
            let mut command = Command::new("sleep");
            command.arg("60");
            let mut root = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
            let mut spec: crate::runtime::background::BackgroundRunSpec = serde_json::from_str(
                include_str!("../../../testdata/legacy-background-spec.json"),
            ).expect("spec");
            spec.task_id = task.id.to_string();
            spec.worker_pid = Some(root.id());
            spec.agent_pid = Some(root.id());
            spec.agent_started = started;
            crate::runtime::background::save_spec(&spec).expect("save spec");
            action(&store, task.id.as_str()).expect("stop");
            std::thread::sleep(std::time::Duration::from_millis(100));
            let survived = root.child_mut().try_wait().expect("wait").is_none();
            root.force_kill();
            assert!(survived, "stop must reject a reused PID or missing launch identity");
        }
    }
}

#[cfg(unix)]
#[test]
fn detached_stop_and_force_kill_signal_matching_agent_identity() {
    let _permit = test_subprocess::acquire();
    for action in [stop, kill] {
        let home = crate::support::test_env::TestHome::new();
        let store = Arc::new(Store::open_memory().expect("store"));
        let mut task = running_task("t-stop-owned", home.path(), home.path(), "unused");
        task.worktree_path = None;
        task.read_only = true;
        store.insert_task(&task).expect("task");
        let mut command = Command::new("sleep");
        command.arg("60");
        let mut root = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("agent");
        let mut spec: crate::runtime::background::BackgroundRunSpec = serde_json::from_str(
            include_str!("../../../testdata/legacy-background-spec.json"),
        ).expect("spec");
        spec.task_id = task.id.to_string();
        spec.worker_pid = None;
        spec.agent_pid = Some(root.id());
        spec.agent_started = crate::runtime::process::monitor::activity::root_started(root.id());
        assert!(spec.agent_started.is_some());
        crate::runtime::background::save_spec(&spec).expect("save spec");
        action(&store, task.id.as_str()).expect("stop");
        assert!(!root.child_mut().wait().expect("wait").success(), "owned agent must be killed");
    }
}
