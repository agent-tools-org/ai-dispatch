// Concurrent input changes through the complete read-only post-run lifecycle.
// Covers restrictive native launches, advisory Claude and historical missing provenance.

use super::*;
use crate::agent::{Agent, CommandContext, RunOpts, codex::CodexAgent};

fn commit_during_audit(f: &Fixture) {
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    fs::write(dir.join("concurrent.txt"), "concurrent commit\n").unwrap();
    git_output(dir, &["add", "concurrent.txt"]);
    git_output(dir, &["commit", "-m", "concurrent change"]);
}

fn record_native(f: &Fixture) {
    let cmd = CodexAgent
        .build_command_with_context(
            "audit",
            &RunOpts {
                dir: f.args.dir.clone(),
                output: None,
                result_file: None,
                model: None,
                budget: false,
                read_only: true,
                sandbox: false,
                context_files: vec![],
                session_id: None,
                env: None,
                env_forward: None,
            },
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![PathBuf::from(f.args.artifact_staging.as_ref().unwrap())],
            },
        )
        .unwrap();
    crate::runtime::read_only_launch::record(&f.store, &f.id, &cmd, "app-server").unwrap();
}

#[tokio::test]
async fn enforced_read_only_concurrent_commit_delivers_report_and_finishes_done() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, false, false);
    record_native(&f);
    commit_during_audit(&f);
    f.settle().await;
    assert_eq!(
        f.store.get_task(f.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Done
    );
    assert_eq!(
        fs::read_to_string(f.args.result_file.as_ref().unwrap()).unwrap(),
        REPORT
    );
    let events = f.store.get_events(f.id.as_str()).unwrap();
    let warning = events
        .iter()
        .find(|event| event.detail.contains("audited inputs changed during the run"))
        .unwrap();
    assert!(warning.detail.contains("concurrent.txt"));
    assert!(warning.detail.contains(String::from_utf8_lossy(&f.head).trim()));
    let current = git_output(Path::new(f.args.dir.as_ref().unwrap()), &["rev-parse", "HEAD"]);
    assert!(warning.detail.contains(String::from_utf8_lossy(&current).trim()));
    assert!(
        !events
            .iter()
            .any(|event| event.detail.starts_with("Read-only violation"))
    );
}

#[tokio::test]
async fn claude_read_only_concurrent_commit_still_finishes_failed() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), true, false, false);
    f.args.agent_name = "claude".into();
    let report = Path::new(f.args.result_file.as_ref().unwrap());
    fs::create_dir_all(report.parent().unwrap()).unwrap();
    fs::write(report, REPORT).unwrap();
    f.store
        .db()
        .execute("UPDATE tasks SET agent = 'claude' WHERE id = ?1", [f.id.as_str()])
        .unwrap();
    crate::runtime::read_only_launch::record(&f.store, &f.id, &Command::new("claude"), "exec").unwrap();
    commit_during_audit(&f);
    f.settle().await;
    assert_eq!(
        f.store.get_task(f.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert!(
        f.store
            .latest_error(f.id.as_str())
            .unwrap()
            .contains("Read-only violation")
    );
}

#[tokio::test]
async fn codex_read_only_concurrent_commit_without_launch_provenance_finishes_failed() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, false, false);
    commit_during_audit(&f);
    f.settle().await;
    assert_eq!(
        f.store.get_task(f.id.as_str()).unwrap().unwrap().status,
        TaskStatus::Failed
    );
    assert!(
        f.store
            .latest_error(f.id.as_str())
            .unwrap()
            .contains("Read-only violation")
    );
}
