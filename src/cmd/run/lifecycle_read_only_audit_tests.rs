// Audit verdict regressions after a read-only settlement violation.
// Uses the sibling lifecycle fixture and an isolated AIC executable.
// Deps: parent Fixture, aic test lock and task Store.

use super::*;

#[tokio::test]
async fn read_only_violation_records_audit_verdict_when_report_exists() {
    let _permit = test_subprocess::acquire();
    let _env = crate::aic::test_env_lock();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), true, false, false);
    let aic = home.path().join("aic");
    executable(&aic, "#!/bin/sh\nprintf 'report: /tmp/audit.md\\n'\nexit 0\n");
    unsafe {
        std::env::set_var("AIC_TEST_BINARY", &aic);
    }
    f.args.audit = true;
    f.settle().await;
    unsafe {
        std::env::remove_var("AIC_TEST_BINARY");
    }
    f.assert_failed_and_reviewable(home.path());
    let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
    assert_eq!(task.audit_verdict.as_deref(), Some("pass"));
    assert_eq!(task.audit_report_path.as_deref(), Some("/tmp/audit.md"));
    let events = f.store.get_events(f.id.as_str()).unwrap();
    assert_eq!(
        events.iter().filter(|e| e.event_kind == EventKind::Error).count(),
        1
    );
}

#[tokio::test]
async fn read_only_settlement_detects_head_and_ref_changes_in_clean_checkout() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), false, false, false);
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    assert!(
        Command::new("git")
            .current_dir(dir)
            .args(["commit", "--allow-empty", "-m", "audit commit"])
            .output()
            .unwrap()
            .status
            .success()
    );
    f.settle().await;
    let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    let error = f.store.latest_error(f.id.as_str()).unwrap();
    assert!(error.contains(".git/HEAD"), "{error}");
    assert!(error.contains(".git/refs/heads/fix/read-only-backup"), "{error}");
}
