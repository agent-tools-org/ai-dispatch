// Backup module tests: settings resolution, bundle packing, and the lifecycle
// hook driven through a fake target. No gws, no network.

use super::config::{expand_folder, parse_cli_spec, resolve, Artifact};
use super::lifecycle_tests::settle;
use super::*;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
use chrono::Local;
use std::cell::RefCell;
use std::fs;

fn task(id: &str) -> Task {
    Task {
        id: TaskId(id.to_string()),
        agent: AgentKind::Codex,
        custom_agent_name: None,
        prompt: "prompt".to_string(),
        resolved_prompt: None,
        category: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: None,
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None, project_id: Some("proj".to_string()),
        worktree_path: None, effective_dir: None,
        worktree_branch: Some("feat/x".to_string()),
        final_head_sha: Some("0123456789abcdef".to_string()),
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

fn project(toml: &str) -> BackupProjectConfig {
    toml::from_str(toml).unwrap()
}

#[test]
fn cli_spec_splits_target_and_folder() {
    assert_eq!(parse_cli_spec("gdrive").unwrap(), ("gdrive".into(), None));
    assert_eq!(parse_cli_spec("gdrive:audit/2026Q2/").unwrap(), ("gdrive".into(), Some("audit/2026Q2".into())));
    assert!(parse_cli_spec(":x").is_err());
}

#[test]
fn resolve_prefers_cli_then_project_then_global_folder() {
    let global: BackupGlobalConfig = toml::from_str("[gdrive]\nfolder = 'global/{project}'").unwrap();
    let proj = project("target = 'gdrive'\nfolder = 'proj/{date}'\ninclude = ['export']\non = ['fail']");

    let cli = resolve(Some("gdrive:cli"), false, Some(&proj), &global).unwrap().unwrap();
    assert_eq!(cli.folder, "cli");
    assert_eq!(cli.include, vec![Artifact::Export]);
    assert_eq!(cli.on, vec![Trigger::Fail]);

    let from_project = resolve(None, false, Some(&proj), &global).unwrap().unwrap();
    assert_eq!(from_project.folder, "proj/{date}");

    let bare = resolve(Some("gdrive"), false, None, &global).unwrap().unwrap();
    assert_eq!(bare.folder, "global/{project}");
    assert_eq!(bare.include, vec![Artifact::Export, Artifact::Diff, Artifact::Transcript]);
    assert_eq!(bare.on, vec![Trigger::Complete, Trigger::Fail]);

    let unconfigured = resolve(None, false, None, &BackupGlobalConfig::default()).unwrap();
    assert!(unconfigured.is_none());
    assert!(resolve(Some("gdrive"), true, Some(&proj), &global).unwrap().is_none());
    assert!(resolve(None, false, Some(&project("target='gdrive'\ninclude=['logs']")), &global).is_err());
    let cancelled = resolve(None, false, Some(&project("target='gdrive'\non=['cancelled']")), &global);
    assert!(cancelled.unwrap_err().to_string().contains("unknown backup trigger 'cancelled'"));
}

#[test]
fn folder_templates_expand_task_fields() {
    let folder = expand_folder("/x/{project}/{branch}/{task_id}/", &task("t-42"));
    assert_eq!(folder, "x/proj/feat/x/t-42");
    let dated = expand_folder("{date}", &task("t-42"));
    assert_eq!(dated, Local::now().format("%Y-%m-%d").to_string());
}

#[test]
fn bundle_packs_export_diff_and_transcript() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    let task = task("t-bundle");
    store.insert_task(&task).unwrap();
    let log = crate::paths::log_path("t-bundle");
    fs::create_dir_all(log.parent().unwrap()).unwrap();
    fs::write(&log, "{\"line\":1}\n").unwrap();

    let bundle = bundle::build(&store, &task, &Artifact::ALL).unwrap();

    assert_eq!(bundle.file_name, "t-bundle-0123456.tar.gz");
    let listing = std::process::Command::new("tar").arg("-tzf").arg(&bundle.path).output().unwrap();
    let listing = String::from_utf8_lossy(&listing.stdout).to_string();
    for name in ["export.md", "diff.patch", "transcript.jsonl"] {
        assert!(listing.contains(name), "{listing}");
    }
    let dir = bundle.dir.clone();
    drop(bundle);
    assert!(!dir.exists(), "temp dir must be removed after upload");
}

#[test]
fn bundle_without_any_artifact_is_an_error() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    let task = task("t-empty");
    store.insert_task(&task).unwrap();

    let err = match bundle::build(&store, &task, &[Artifact::Transcript]) {
        Ok(_) => panic!("expected an error"),
        Err(err) => err.to_string(),
    };

    assert!(err.contains("no artifacts"), "{err}");
}

struct FakeTarget {
    dests: RefCell<Vec<BackupDest>>,
    fail: bool,
}

impl BackupTarget for FakeTarget {
    fn name(&self) -> &str {
        "fake"
    }
    fn upload(&self, bundle: &Path, dest: &BackupDest) -> Result<BackupRef> {
        assert!(bundle.is_file());
        self.dests.borrow_mut().push(dest.clone());
        if self.fail {
            anyhow::bail!("network down");
        }
        Ok(BackupRef { id: "id1".into(), url: "https://example.test/id1".into() })
    }
}

fn settings() -> BackupSettings {
    BackupSettings {
        target: "fake".into(),
        folder: "aid-backups/{project}".into(),
        include: vec![Artifact::Export],
        on: vec![Trigger::Complete],
        binary: None,
    }
}

#[test]
fn successful_backup_records_url_and_milestone() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-ok")).unwrap();
    let target = FakeTarget { dests: RefCell::new(Vec::new()), fail: false };

    let url = run_backup_with(&store, "t-ok", &settings(), &target);

    assert_eq!(url.as_deref(), Some("https://example.test/id1"));
    assert_eq!(store.backup_url("t-ok").unwrap().as_deref(), Some("https://example.test/id1"));
    let dests = target.dests.borrow();
    assert_eq!(dests[0].folder, "aid-backups/proj");
    assert_eq!(dests[0].file_name, "t-ok-0123456.tar.gz");
    let events = store.get_events("t-ok").unwrap();
    assert!(events.iter().any(|e| e.event_kind == EventKind::Milestone && e.detail.contains("Backup uploaded")));
    assert_eq!(store.get_task("t-ok").unwrap().unwrap().status, TaskStatus::Done);
}

#[test]
fn failed_backup_warns_and_leaves_task_untouched() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-bad")).unwrap();
    let target = FakeTarget { dests: RefCell::new(Vec::new()), fail: true };

    let url = run_backup_with(&store, "t-bad", &settings(), &target);

    assert!(url.is_none());
    assert!(store.backup_url("t-bad").unwrap().is_none());
    let events = store.get_events("t-bad").unwrap();
    let warning = events.iter().find(|e| e.detail.contains("Backup failed")).unwrap();
    assert_eq!(warning.event_kind, EventKind::Milestone, "never an error event");
    assert!(warning.detail.contains("network down"));
    assert_eq!(warning.metadata.as_ref().unwrap()["backup"], "failed");
    assert!(events.iter().all(|e| e.event_kind != EventKind::Error));
    assert_eq!(store.get_task("t-bad").unwrap().unwrap().status, TaskStatus::Done);
}

fn repo_with_backup(toml: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    fs::create_dir_all(repo.path().join(".git")).unwrap();
    fs::create_dir_all(repo.path().join(".aid")).unwrap();
    fs::write(repo.path().join(".aid/project.toml"), format!("[project]\nid = 'proj'\n{toml}")).unwrap();
    repo
}

#[test]
fn settled_task_reads_project_config_and_reports_unknown_target() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let repo = repo_with_backup("[backup]\ntarget = 'nope'\non = ['complete']");
    let store = Store::open_memory().unwrap();
    let mut task = task("t-proj");
    task.repo_path = Some(repo.path().display().to_string());
    store.insert_task(&task).unwrap();
    // Persisted args with neither flag: the project `[backup]` decides.
    let args = crate::cmd::run::RunArgs::default();
    store.update_task_dispatch_args("t-proj", &args.dispatch_args_json().unwrap()).unwrap();

    settle(&store, "t-proj", TaskStatus::Failed);
    assert!(store.get_events("t-proj").unwrap().is_empty(), "fail is not in `on`");

    settle(&store, "t-proj", TaskStatus::Done);
    let events = store.get_events("t-proj").unwrap();
    assert!(events.iter().any(|e| e.detail.contains("unknown backup target 'nope'")), "{events:?}");
}

#[test]
fn settled_task_honors_no_backup_and_ignores_unconfigured_tasks() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let repo = repo_with_backup("[backup]\ntarget = 'nope'");
    let store = Store::open_memory().unwrap();
    let mut off = task("t-off");
    off.repo_path = Some(repo.path().display().to_string());
    store.insert_task(&off).unwrap();
    let args = crate::cmd::run::RunArgs { no_backup: true, ..Default::default() };
    store.update_task_dispatch_args("t-off", &args.dispatch_args_json().unwrap()).unwrap();
    store.insert_task(&task("t-plain")).unwrap();

    settle(&store, "t-off", TaskStatus::Done);
    settle(&store, "t-plain", TaskStatus::Done);
    settle(&store, "t-missing", TaskStatus::Done);

    assert!(store.get_events("t-off").unwrap().is_empty());
    assert!(store.get_events("t-plain").unwrap().is_empty());
}

#[test]
fn cli_backup_flag_enables_backup_without_project_config() {
    let home = tempfile::tempdir().unwrap();
    let _guard = AidHomeGuard::set(home.path());
    let store = Store::open_memory().unwrap();
    store.insert_task(&task("t-cli")).unwrap();
    let args = crate::cmd::run::RunArgs { backup: Some("nope:x".into()), ..Default::default() };
    store.update_task_dispatch_args("t-cli", &args.dispatch_args_json().unwrap()).unwrap();

    settle(&store, "t-cli", TaskStatus::Stopped);
    assert!(store.get_events("t-cli").unwrap().is_empty(), "a stopped task is never backed up");
    settle(&store, "t-cli", TaskStatus::Done);

    let events = store.get_events("t-cli").unwrap();
    assert!(events.iter().any(|e| e.detail.contains("unknown backup target 'nope'")), "{events:?}");
}
