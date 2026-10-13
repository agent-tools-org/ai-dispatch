// Scope-cut delivery regressions for trusted paths, provenance and source identity.
// Uses the existing lifecycle fixture and a test-only source replacement callback.
// Deps: read_only_delivery, Store and shared CLI/UI output readers.

use super::*;
use crate::cmd::run::read_only_delivery;

#[test]
fn scope_cut_fix_rejects_task_home_inside_repository_or_git_common_directory() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), false, false, false);
    for root in [
        PathBuf::from(f.args.dir.as_ref().unwrap()),
        f._repo.path().join(".git"),
    ] {
        let _nested = paths::AidHomeGuard::set(&root.join("aid-home"));
        let mut args = f.args.clone();
        let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
        let error = read_only_delivery::prepare(&mut args, AgentKind::Codex, &task)
            .expect_err("read-only dispatch must reject a protected task directory");
        assert!(error.to_string().contains("task directory"), "{error}");
    }
}

#[tokio::test]
async fn scope_cut_fix_missing_or_refused_staging_never_selects_old_reports() {
    let _permit = test_subprocess::acquire();
    for output in [false, true] {
        for refused in [false, true] {
            let home = tempfile::tempdir().unwrap();
            let _guard = paths::AidHomeGuard::set(home.path());
            let mut f = Fixture::new(home.path(), false, false, false);
            fs::write(
                Path::new(f.args.dir.as_ref().unwrap()).join("base.txt"),
                "base\n",
            )
            .unwrap();
            let destination = home.path().join("old-report.md");
            fs::write(&destination, "STALE REPORT").unwrap();
            let official = paths::task_dir(f.id.as_str()).join("result.md");
            fs::create_dir_all(official.parent().unwrap()).unwrap();
            fs::write(&official, "STALE OFFICIAL REPORT").unwrap();
            let name = if output {
                f.args.result_file = None;
                f.args.output = Some(destination.display().to_string());
                f.store
                    .update_output_path(f.id.as_str(), destination.to_str().unwrap())
                    .unwrap();
                "agent-output.txt"
            } else {
                f.args.result_file = Some(destination.display().to_string());
                "agent-result.md"
            };
            if refused {
                std::os::unix::fs::symlink(&destination, staged(&f, name)).unwrap();
            }
            f.settle().await;
            let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
            for text in [
                crate::cmd::show::output_text(&f.store, f.id.as_str()).unwrap(),
                crate::task_view::read_output(&task),
            ] {
                assert!(text.contains("not delivered by this run"), "{text}");
                assert!(!text.contains("STALE"), "{text}");
            }
            assert_delivery_failed(&f);
            assert_eq!(fs::read_to_string(destination).unwrap(), "STALE REPORT");
        }
    }
}

#[tokio::test]
async fn scope_cut_fix_official_report_copy_failure_records_task_event() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, false, false);
    fs::write(
        Path::new(f.args.dir.as_ref().unwrap()).join("base.txt"),
        "base\n",
    )
    .unwrap();
    fs::create_dir_all(paths::task_dir(f.id.as_str()).join("result.md")).unwrap();
    f.settle().await;
    assert_delivery_failed(&f);
}

#[tokio::test]
async fn scope_cut_fix_replacement_after_validation_is_not_published_to_any_sink() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, false, false);
    fs::write(
        Path::new(f.args.dir.as_ref().unwrap()).join("base.txt"),
        "base\n",
    )
    .unwrap();
    let source = staged(&f, "agent-result.md");
    let replacement = home.path().join("replacement.md");
    fs::write(&replacement, "REPLACEMENT MUST NOT BE PUBLISHED").unwrap();
    read_only_delivery::AFTER_VALIDATE.with(|hook| {
        *hook.borrow_mut() = Some(Box::new(move || {
            fs::remove_file(&source).unwrap();
            std::os::unix::fs::symlink(replacement, source).unwrap();
        }));
    });
    f.settle().await;
    assert_eq!(
        fs::read_to_string(f.args.result_file.as_ref().unwrap()).unwrap(),
        REPORT
    );
    assert_eq!(
        fs::read_to_string(paths::task_dir(f.id.as_str()).join("result.md")).unwrap(),
        REPORT
    );
}

#[tokio::test]
async fn scope_cut_fix_hard_linked_staged_report_is_refused() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, false, false);
    fs::write(
        Path::new(f.args.dir.as_ref().unwrap()).join("base.txt"),
        "base\n",
    )
    .unwrap();
    fs::hard_link(staged(&f, "agent-result.md"), home.path().join("alias.md")).unwrap();
    f.settle().await;
    assert_delivery_failed(&f);
    assert!(!Path::new(f.args.result_file.as_ref().unwrap()).exists());
}
