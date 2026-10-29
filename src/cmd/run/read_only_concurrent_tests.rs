// Concurrent commit regressions at the read-only settlement boundary.
// Exercises native launch evidence independently of task agent labels and requested flags.

use super::*;
use crate::agent::{Agent, CommandContext, RunOpts, codex::CodexAgent};
use crate::runtime::read_only_launch;

fn record_native(store: &Store, id: &TaskId, repo: &Path, scratch: &Path) {
    let cmd = CodexAgent
        .build_command_with_context(
            "audit",
            &RunOpts {
                dir: Some(repo.display().to_string()),
                read_only: true,
                ..Default::default()
            },
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![scratch.to_path_buf()],
            },
        )
        .unwrap();
    read_only_launch::record(store, id, &cmd, "exec").unwrap();
}

fn concurrent_commit(repo: &Path) -> String {
    fs::write(repo.join("concurrent.txt"), "changed under audit\n").unwrap();
    git(repo, &["add", "concurrent.txt"]);
    git(repo, &["commit", "-m", "concurrent change"]);
    String::from_utf8(
        Command::new("git")
            .current_dir(repo)
            .args(["rev-parse", "HEAD"])
            .output()
            .unwrap()
            .stdout,
    )
    .unwrap()
    .trim()
    .into()
}

#[test]
fn enforced_read_only_concurrent_commit_keeps_done_and_warns_with_snapshot_evidence() {
    let (repo, store, id, before) = fixture();
    let scratch = tempfile::tempdir().unwrap();
    record_native(&store, &id, repo.path(), scratch.path());
    let baseline_head = snapshot_head(&before).unwrap();
    let current_head = concurrent_commit(repo.path());
    assert!(matches!(
        settle(
            &store,
            &id,
            &RunArgs::default(),
            repo.path().to_str().unwrap(),
            Some(&before)
        )
        .unwrap(),
        DirtyWorktreeAction::Continue
    ));
    assert_eq!(
        store.get_task(id.as_str()).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert!(store.latest_error(id.as_str()).is_none());
    let warning = store
        .get_events(id.as_str())
        .unwrap()
        .into_iter()
        .find(|event| {
            event
                .metadata
                .as_ref()
                .is_some_and(|meta| meta["warning"] == "read_only_concurrent_change")
        })
        .unwrap();
    assert!(warning.detail.contains("audited inputs changed during the run"));
    assert!(warning.detail.contains(&baseline_head));
    assert!(warning.detail.contains(&current_head));
    assert!(warning.detail.contains("concurrent.txt"));
    let metadata = warning.metadata.unwrap();
    assert_eq!(metadata["baseline_head"], baseline_head);
    assert_eq!(metadata["current_head"], current_head);
    assert_eq!(metadata["input_stability"], "unknown");
    assert!(
        metadata["changed_paths"]
            .as_array()
            .unwrap()
            .contains(&serde_json::json!("concurrent.txt"))
    );
}

#[test]
fn claude_and_missing_native_provenance_still_fail_on_concurrent_commit() {
    for agent in ["claude", "codex"] {
        let (repo, store, id, before) = fixture();
        store
            .db()
            .execute("UPDATE tasks SET agent = ?1 WHERE id = ?2", [agent, id.as_str()])
            .unwrap();
        if agent == "claude" {
            read_only_launch::record(&store, &id, &Command::new("claude"), "exec").unwrap();
        }
        concurrent_commit(repo.path());
        let args = RunArgs {
            read_only: true,
            agent_name: agent.into(),
            ..Default::default()
        };
        assert!(matches!(
            settle(&store, &id, &args, repo.path().to_str().unwrap(), Some(&before)).unwrap(),
            DirtyWorktreeAction::Failed
        ));
        assert_eq!(
            store.get_task(id.as_str()).unwrap().unwrap().status,
            TaskStatus::Failed
        );
        assert!(
            store
                .latest_error(id.as_str())
                .unwrap()
                .contains("Read-only violation")
        );
    }
}

#[test]
fn enforced_read_only_warning_does_not_override_failed_agent_result() {
    let (repo, store, id, before) = fixture();
    let scratch = tempfile::tempdir().unwrap();
    record_native(&store, &id, repo.path(), scratch.path());
    crate::task::lifecycle::mark_failed(&store, &id).unwrap();
    concurrent_commit(repo.path());
    assert!(matches!(
        settle(
            &store,
            &id,
            &RunArgs::default(),
            repo.path().to_str().unwrap(),
            Some(&before)
        )
        .unwrap(),
        DirtyWorktreeAction::Continue
    ));
    assert_eq!(
        store.get_task(id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
}

#[test]
fn enforced_read_only_missing_snapshot_still_fails() {
    let (repo, store, id, _) = fixture();
    let scratch = tempfile::tempdir().unwrap();
    record_native(&store, &id, repo.path(), scratch.path());
    assert!(matches!(
        settle(
            &store,
            &id,
            &RunArgs::default(),
            repo.path().to_str().unwrap(),
            None
        )
        .unwrap(),
        DirtyWorktreeAction::Failed
    ));
    assert!(
        store
            .latest_error(id.as_str())
            .unwrap()
            .contains("Dispatch snapshot is unavailable")
    );
}

#[test]
fn enforced_read_only_snapshot_capture_error_still_fails() {
    let (repo, store, id, before) = fixture();
    let scratch = tempfile::tempdir().unwrap();
    record_native(&store, &id, repo.path(), scratch.path());
    fs::write(repo.path().join(".git/index"), b"broken index").unwrap();
    assert!(matches!(
        settle(
            &store,
            &id,
            &RunArgs::default(),
            repo.path().to_str().unwrap(),
            Some(&before)
        )
        .unwrap(),
        DirtyWorktreeAction::Failed
    ));
    assert!(
        store
            .latest_error(id.as_str())
            .unwrap()
            .contains("Read-only enforcement failed")
    );
}
