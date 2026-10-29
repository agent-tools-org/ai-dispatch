// A killed publication failure must leave the live worker's completion barrier intact.
// Process ownership stays with this fixture; receipt probes do not reap either child.
use super::*;
use crate::{store::Store, types::*, runtime::background};
use std::sync::Arc;

#[cfg(unix)]
#[test]
fn caller_owned_publication_kill_retains_the_live_worker_spec_through_lifecycle() {
    let _permit = crate::support::test_subprocess::acquire();
    let _home = crate::support::test_env::TestHome::with_dirs();
    let store = Arc::new(Store::open_memory().expect("store"));
    let id = "t-live-worker-publication";
    store
        .insert_task(&Task::test_fixture(TaskId(id.into()), TaskStatus::Running))
        .expect("task");
    let mut command = Command::new("sh");
    command
        .args(["-c", "read line", "__run-task", id])
        .stdin(std::process::Stdio::piped());
    let _worker = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("worker");
    background::save_spec(
        &serde_json::from_value(serde_json::json!({
        "task_id":id,"worker_pid":_worker.id()}))
        .expect("spec"),
    )
    .expect("save");
    store.test_caller_owned(id);
    store.mark_worker_spawned(id).expect("spawn evidence");
    let result: Result<()> = store.with_running_task(true, id, |_pid| {
        let mut command = Command::new("sleep");
        command.arg("60");
        let mut agent = crate::runtime::process::guard::ProcessGuard::spawn(&mut command)?;
        agent.force_kill();
        agent.child_mut().wait()?;
        crate::cmd::execution_receipt::group_gone(agent.id())?;
        Err(AgentPublicationFailure {
            source: anyhow::anyhow!("publication failed"),
            confirmed_gone: true,
        }
        .into())
    });
    assert!(result.is_err());
    store
        .update_task_status(id, TaskStatus::Failed)
        .expect("post-lifecycle still running");
    assert!(crate::support::paths::job_path(id).exists());
    let receipt = crate::cmd::execution_receipt::receipt(&store, id).expect("receipt");
    assert_eq!(receipt["worker"]["state"], "running");
    assert_eq!(receipt["settled"], false);
}

#[cfg(unix)]
#[test]
fn caller_owned_publication_cleanup_never_signals_a_mismatched_launch_identity() {
    let _permit = crate::support::test_subprocess::acquire();
    let mut command = Command::new("sleep");
    command.arg("60");
    let mut foreign = crate::runtime::process::guard::ProcessGuard::spawn(&mut command).expect("foreign");
    let error = abort_agent_publication(Some((foreign.id(), Some("previous launch".into()))),
        anyhow::anyhow!("publication failed"));
    assert!(!error.downcast_ref::<AgentPublicationFailure>().expect("publication error").confirmed_gone);
    assert!(foreign.child_mut().try_wait().expect("foreign remains alive").is_none());
}

#[cfg(unix)]
#[test]
fn publication_abort_kills_verified_group_including_another_member() {
    use std::os::unix::process::CommandExt;
    let _permit = crate::support::test_subprocess::acquire();
    let mut leader_command = Command::new("sleep");
    leader_command.arg("60");
    let mut leader = crate::runtime::process::guard::ProcessGuard::spawn(&mut leader_command)
        .expect("leader");
    let pid = leader.id();
    let started = crate::runtime::process::monitor::activity::root_started(pid).expect("identity");
    let mut member = Command::new("sleep").arg("60").process_group(pid as i32)
        .spawn().expect("group member");
    let error = abort_agent_publication(Some((pid, Some(started))), anyhow::anyhow!("publication"));
    assert!(error.downcast_ref::<AgentPublicationFailure>().expect("failure").confirmed_gone);
    assert!(!leader.child_mut().wait().expect("leader wait").success());
    assert!(!member.wait().expect("member wait").success());
}
