// Native permission provenance regressions without launching a model.
// Checks canonical policy evidence, fallback/resume preservation and stale-proof rejection.

use super::*;
use crate::agent::{Agent, CommandContext, RunOpts, codex::CodexAgent};
use crate::types::{EventKind, TaskEvent};

#[path = "read_only_storage_tests.rs"]
mod storage_tests;

fn command(repo: &Path, scratch: &Path, resume: bool) -> Command {
    CodexAgent
        .build_command_with_context(
            "audit",
            &RunOpts {
                dir: Some(repo.display().to_string()),
                read_only: true,
                session_id: resume.then(|| "session".into()),
                ..Default::default()
            },
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![scratch.to_path_buf()],
            },
        )
        .unwrap()
}

#[test]
fn native_read_only_provenance_records_canonical_policy_for_fresh_and_resumed_exec() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    for resume in [false, true] {
        let cmd = command(repo.path(), scratch.path(), resume);
        let proof = profile(&cmd).unwrap();
        assert_eq!(proof.identity, "aid-read-only");
        assert_eq!(proof.approval_policy, "never");
        assert_eq!(proof.writable_roots, vec![scratch.path().canonicalize().unwrap()]);
        assert_eq!(proof.cwd, repo.path().canonicalize().unwrap());
    }
}

#[test]
fn native_read_only_provenance_survives_exec_fallback_and_resume_refusal() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();
    let task = TaskId("t-policy-fallback".into());
    store
        .insert_waiting_task(
            task.as_str(),
            "codex",
            "audit",
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            false,
        )
        .unwrap();
    let cmd = command(repo.path(), scratch.path(), true);
    let resumed = super::super::codex_app_server::exec_fallback(&cmd, &store, &task).unwrap();
    assert!(profile(&resumed).is_some());
    store
        .insert_event(&TaskEvent::new(task.clone(), EventKind::Milestone, "resume refused".into())
            .with_metadata(Some(serde_json::json!({
                "codex_app_server_fallback": true, "reason": "Codex thread/resume: refused",
            }))))
        .unwrap();
    let fresh = super::super::codex_app_server::exec_fallback(&cmd, &store, &task).unwrap();
    assert!(profile(&fresh).is_some());
    assert!(!fresh.get_args().any(|arg| arg == "resume" || arg == "session"));
}

#[test]
fn native_read_only_provenance_rejects_overrides_wrappers_and_missing_builder_identity() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    for setting in [
        "approval_policy=\"on-request\"",
        "default_permissions=\"aid-write\"",
        "permissions.aid-read-only.extends=\":workspace\"",
        "mcp_servers.writer.command=\"writer\"",
    ] {
        let mut cmd = command(repo.path(), scratch.path(), false);
        cmd.args(["-c", setting]);
        assert!(profile(&cmd).is_none(), "{setting}");
    }
    let mut missing = command(repo.path(), scratch.path(), false);
    missing.env_remove("AID_NATIVE_READ_ONLY_PROFILE");
    assert!(profile(&missing).is_none());
    let native = command(repo.path(), scratch.path(), false);
    let mut wrapper = Command::new("custom-codex");
    wrapper
        .args(native.get_args())
        .current_dir(repo.path())
        .env("AID_NATIVE_READ_ONLY_PROFILE", "aid-read-only");
    assert!(profile(&wrapper).is_none());
}

#[test]
fn native_read_only_provenance_reset_and_unknown_launch_invalidate_previous_attempt() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let store = Store::open_memory().unwrap();
    let task = TaskId("t-policy-attempt".into());
    store
        .insert_waiting_task(
            task.as_str(),
            "codex",
            "audit",
            None,
            None,
            None,
            None,
            None,
            None,
            true,
            false,
        )
        .unwrap();
    let dir = repo.path().to_str().unwrap();
    assert!(!enforced(&store, &task, dir).unwrap());
    let cmd = command(repo.path(), scratch.path(), false);
    record(&store, &task, &cmd, "app-server").unwrap();
    assert!(enforced(&store, &task, dir).unwrap());
    reset(&store, &task).unwrap();
    assert!(!enforced(&store, &task, dir).unwrap());
    record(&store, &task, &cmd, "exec").unwrap();
    assert!(enforced(&store, &task, dir).unwrap());
    record(&store, &task, &Command::new("claude"), "exec").unwrap();
    assert!(!enforced(&store, &task, dir).unwrap());
}

#[test]
fn native_read_only_provenance_rejects_overlapping_writable_root() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let mut cmd = command(repo.path(), scratch.path(), false);
    let root = repo.path().canonicalize().unwrap();
    let filesystem = toml::Value::Table(toml::Table::from_iter([(
        root.to_str().unwrap().to_string(),
        toml::Value::String("write".into()),
    )]));
    cmd.args([
        "-c",
        &format!(
            "permissions={{ aid-read-only = {{ extends = \":read-only\", filesystem = {filesystem} }} }}",
        ),
    ]);
    assert!(profile(&cmd).is_none());
}

#[test]
fn native_read_only_provenance_requires_ignore_user_config() {
    let repo = tempfile::tempdir().unwrap();
    let scratch = tempfile::tempdir().unwrap();
    let native = command(repo.path(), scratch.path(), false);
    let mut cmd = Command::new(native.get_program());
    cmd.args(native.get_args().filter(|arg| *arg != "--ignore-user-config"));
    cmd.current_dir(repo.path());
    for (key, value) in native.get_envs() {
        if let Some(value) = value {
            cmd.env(key, value);
        }
    }
    assert!(profile(&cmd).is_none());
}
