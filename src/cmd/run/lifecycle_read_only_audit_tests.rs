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

#[tokio::test]
async fn fix_round_one_read_only_delivers_staged_required_report_outside_repository() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), false, false, false);
    f.args.agent_name = "codex".into();
    f.args.result_file_required = Some(true);
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    let task_dir = paths::task_dir(f.id.as_str());
    fs::create_dir_all(&task_dir).unwrap();
    fs::write(task_dir.join("agent-result.md"), REPORT).unwrap();
    f.settle().await;
    assert_eq!(fs::read_to_string(f.args.result_file.as_ref().unwrap()).unwrap(), REPORT);
    assert_eq!(fs::read_to_string(task_dir.join("result.md")).unwrap(), REPORT);
    assert_eq!(f.store.get_task(f.id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
}

#[tokio::test]
async fn fix_round_one_read_only_delivers_staged_output_to_explicit_absolute_path() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), false, false, false);
    f.args.agent_name = "codex".into();
    f.args.result_file = None;
    let destination = home.path().join("requested/final.txt");
    f.args.output = Some(destination.display().to_string());
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    let task_dir = paths::task_dir(f.id.as_str());
    fs::create_dir_all(&task_dir).unwrap();
    fs::write(task_dir.join("agent-output.txt"), "final findings\n").unwrap();
    f.settle().await;
    assert_eq!(fs::read_to_string(destination).unwrap(), "final findings\n");
}

#[tokio::test]
async fn fix_round_one_read_only_missing_staged_report_rejects_stale_destination() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), true, false, false);
    f.args.agent_name = "codex".into();
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    fs::remove_file(paths::task_dir(f.id.as_str()).join("agent-result.md")).unwrap();
    let destination = Path::new(f.args.result_file.as_ref().unwrap());
    fs::create_dir_all(destination.parent().unwrap()).unwrap();
    fs::write(destination, REPORT).unwrap();
    f.settle().await;
    let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
    assert_eq!(task.status, TaskStatus::Failed);
    assert_eq!(task.delivery_assessment, Some(crate::types::DeliveryAssessment::MissingFinalDelivery));
    assert_eq!(fs::read_to_string(destination).unwrap(), REPORT);
}

#[tokio::test]
async fn fix_round_two_read_only_refuses_repository_destinations_after_settlement() {
    let _permit = test_subprocess::acquire();
    for destination in ["base.txt", "new/reports/final.md", "git-config", "parent-alias"] {
        let home = tempfile::tempdir().unwrap();
        let _guard = paths::AidHomeGuard::set(home.path());
        let mut f = Fixture::new(home.path(), false, false, false);
        let dir = Path::new(f.args.dir.as_ref().unwrap());
        fs::write(dir.join("base.txt"), "base\n").unwrap();
        let config = f._repo.path().join(".git/config");
        let original_config = fs::read(&config).unwrap();
        let target = match destination {
            "git-config" => config.clone(),
            "parent-alias" => {
                std::os::unix::fs::symlink(dir, home.path().join("alias")).unwrap();
                home.path().join("alias/new/final.md")
            }
            path => dir.join(path),
        };
        f.args.result_file = Some(target.display().to_string());
        let task_dir = paths::task_dir(f.id.as_str());
        fs::write(task_dir.join("agent-result.md"), REPORT).unwrap();
        f.settle().await;
        assert_eq!(fs::read_to_string(dir.join("base.txt")).unwrap(), "base\n");
        assert_eq!(fs::read(config).unwrap(), original_config);
        assert!(!dir.join("new").exists(), "refusal must not create repository directories");
        assert_delivery_failed(&f);
    }
}

#[tokio::test]
async fn fix_round_two_read_only_refuses_destination_symlink_into_repository() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), false, false, false);
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    let destination = home.path().join("report.md");
    std::os::unix::fs::symlink(dir.join("base.txt"), &destination).unwrap();
    f.args.result_file = Some(destination.display().to_string());
    fs::write(paths::task_dir(f.id.as_str()).join("agent-result.md"), REPORT).unwrap();
    f.settle().await;
    assert_eq!(fs::read_to_string(dir.join("base.txt")).unwrap(), "base\n");
    assert!(fs::symlink_metadata(destination).unwrap().file_type().is_symlink());
    assert_delivery_failed(&f);
}

#[tokio::test]
async fn fix_round_two_read_only_output_replaces_external_symlink_atomically() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let mut f = Fixture::new(home.path(), false, false, false);
    let dir = Path::new(f.args.dir.as_ref().unwrap());
    fs::write(dir.join("base.txt"), "base\n").unwrap();
    let target = home.path().join("other.txt");
    fs::write(&target, "untouched\n").unwrap();
    let destination = home.path().join("output.txt");
    std::os::unix::fs::symlink(&target, &destination).unwrap();
    f.args.result_file = None;
    f.args.output = Some(destination.display().to_string());
    fs::write(paths::task_dir(f.id.as_str()).join("agent-output.txt"), REPORT).unwrap();
    f.settle().await;
    assert_eq!(fs::read_to_string(target).unwrap(), "untouched\n");
    assert_eq!(fs::read_to_string(&destination).unwrap(), REPORT);
    assert!(!fs::symlink_metadata(destination).unwrap().file_type().is_symlink());
    assert_eq!(f.store.get_task(f.id.as_str()).unwrap().unwrap().status, TaskStatus::Done);
}

fn assert_delivery_failed(f: &Fixture) {
    assert_eq!(f.store.get_task(f.id.as_str()).unwrap().unwrap().status, TaskStatus::Failed);
    let events = f.store.get_events(f.id.as_str()).unwrap();
    assert!(events.iter().any(|event| event.event_kind == EventKind::Error
        && event.detail.starts_with("Read-only artifact delivery failed:")), "{events:?}");
}
