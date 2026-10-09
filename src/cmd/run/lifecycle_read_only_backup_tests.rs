// Read-only early-stop regressions through the real post-run lifecycle.
// Captures fake-gws archives after an isolated fake agent writes a report and
// violates the checkout; depends on the sibling verify-gate fixture helpers.

use super::{create_worktree, init_repo, prompt_bundle, task};
use crate::cmd::run::{
    RunArgs, run_dirty,
    run_lifecycle::{LifecycleMode, post_run_lifecycle},
};
use crate::{
    paths,
    store::Store,
    test_subprocess,
    types::{AgentKind, EventKind, TaskId, TaskStatus, VerifyStatus},
};
use std::{
    fs,
    os::unix::fs::PermissionsExt,
    path::{Path, PathBuf},
    process::Command,
    sync::Arc,
};

const REPORT: &str = "# Original report\n\nExact evidence: `base.txt` changed.\n";
const VERIFY_NOT_RUN: &str = "Configured verification did not run: dirty worktree settlement failed before verify";

struct Fixture {
    _repo: tempfile::TempDir,
    store: Arc<Store>,
    id: TaskId,
    args: RunArgs,
    baseline: Vec<String>,
    head: Vec<u8>,
    capture: PathBuf,
    // Every fixture uses task id t-read-only-backup in one process, so their backup
    // staging dirs (temp_dir/aid-backup-{task}-{pid}) collide; fixtures run one at a time.
    _serial: std::sync::MutexGuard<'static, ()>,
    log: PathBuf,
}

fn executable(path: &Path, script: &str) {
    fs::write(path, script).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
}

fn git_output(dir: &Path, args: &[&str]) -> Vec<u8> {
    let output = Command::new("git").current_dir(dir).args(args).output().unwrap();
    assert!(output.status.success());
    output.stdout
}

fn run_fake_agent(home: &Path, dir: &Path, write_report: bool) {
    let agent = home.join("fake-agent");
    executable(
        &agent,
        "#!/bin/sh\nset -eu\nif [ \"$1\" = yes ]; then mkdir -p reports; printf '%s' \"$2\" > reports/explicit.md; fi\nprintf 'forbidden edit\\n' > base.txt\n",
    );
    assert!(
        Command::new(agent)
            .current_dir(dir)
            .args([if write_report { "yes" } else { "no" }, REPORT])
            .status()
            .unwrap()
            .success()
    );
}

fn fake_gws(home: &Path, capture: &Path, fail_upload: bool) -> PathBuf {
    let log = home.join("gws.log");
    let binary = home.join("gws");
    let upload_result = if fail_upload {
        "echo 'synthetic upload failure' >&2; exit 2"
    } else {
        ":"
    };
    executable(
        &binary,
        &format!(
            "#!/bin/sh\necho \"$*\" >> '{}'\nfor arg; do last=$arg; done\n\
         case \" $* \" in *' --upload '*) cp \"$last\" '{}'/ || exit 3; {upload_result};; esac\n\
         echo '{{\"id\":\"fake123\"}}'\n",
            log.display(),
            capture.display(),
        ),
    );
    fs::write(
        paths::config_path(),
        format!("[backup.gdrive]\nbinary = '{}'\n", binary.display()),
    )
    .unwrap();
    log
}

impl Fixture {
    fn new(home: &Path, write_report: bool, backup: bool, fail_upload: bool) -> Self {
        let repo = init_repo();
        let wt = create_worktree(repo.path(), "fix/read-only-backup");
        let store = Arc::new(Store::open_memory().unwrap());
        let id = TaskId("t-read-only-backup".into());
        let mut task = task(id.as_str(), repo.path(), &wt, "fix/read-only-backup");
        task.read_only = true;
        let args = RunArgs {
            read_only: true,
            dir: Some(wt.display().to_string()),
            repo: Some(repo.path().display().to_string()),
            result_file: Some("reports/explicit.md".into()),
            result_file_required: Some(write_report),
            verify: backup.then(|| format!("touch '{}'", home.join("verify-ran").display())),
            backup: backup.then(|| "gdrive:audits".into()),
            no_backup: !backup,
            ..Default::default()
        };
        task.verify = args.verify.clone();
        store.insert_task(&task).unwrap();
        store
            .update_task_dispatch_args(id.as_str(), &args.dispatch_args_json().unwrap())
            .unwrap();
        let baseline = run_dirty::capture_baseline(&store, &id, args.dir.as_deref())
            .unwrap()
            .unwrap();
        let head = git_output(&wt, &["rev-parse", "HEAD"]);
        static SERIAL: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let capture = home.join("capture");
        fs::create_dir(&capture).unwrap();
        let log = fake_gws(home, &capture, fail_upload);
        run_fake_agent(home, &wt, write_report);
        Self {
            _repo: repo,
            store,
            id,
            args,
            baseline,
            head,
            capture,
            log,
            _serial: serial,
        }
    }

    async fn settle(&self) {
        let outcome = post_run_lifecycle(
            LifecycleMode::Background,
            &self.store,
            &self.id,
            &self.args,
            AgentKind::Codex,
            "codex",
            self.args.dir.as_ref(),
            self.args.repo.as_ref(),
            self.args.dir.as_ref(),
            None,
            &[],
            &prompt_bundle(),
            TaskStatus::Done,
            Some(&self.baseline),
        )
        .await
        .unwrap();
        assert!(outcome.is_none(), "read-only failure must not dispatch a retry");
    }

    fn assert_failed_and_reviewable(&self, home: &Path) {
        let task = self.store.get_task(self.id.as_str()).unwrap().unwrap();
        assert_eq!(task.status, TaskStatus::Failed);
        assert_eq!(
            task.verify_status,
            if self.args.verify.is_some() {
                VerifyStatus::Failed
            } else {
                VerifyStatus::Skipped
            }
        );
        assert_eq!(task.exit_code, Some(0));
        let events = self.store.get_events(self.id.as_str()).unwrap();
        assert!(
            events.iter().any(|e| e.event_kind == EventKind::Error
                && e.detail.starts_with("Read-only violation:")
                && e.detail.contains("base.txt")
                && !e.detail.contains("reports/explicit.md")),
            "{events:?}"
        );
        assert_eq!(
            events.iter().any(|e| e.detail == VERIFY_NOT_RUN),
            self.args.verify.is_some(),
            "{events:?}"
        );
        assert!(!home.join("verify-ran").exists());
        let dir = Path::new(self.args.dir.as_ref().unwrap());
        assert_eq!(fs::read_to_string(dir.join("base.txt")).unwrap(), "forbidden edit\n");
        assert_eq!(git_output(dir, &["rev-parse", "HEAD"]), self.head);
        assert!(git_output(dir, &["diff", "--cached", "--name-only"]).is_empty());
    }

    fn assert_report(&self) {
        assert_eq!(
            fs::read_to_string(paths::task_dir(self.id.as_str()).join("result.md")).unwrap(),
            REPORT
        );
        assert_eq!(
            fs::read_to_string(Path::new(self.args.dir.as_ref().unwrap()).join("reports/explicit.md")).unwrap(),
            REPORT
        );
        let bundles: Vec<_> = fs::read_dir(&self.capture)
            .unwrap()
            .map(|entry| entry.unwrap().path())
            .collect();
        assert_eq!(bundles.len(), 1);
        let export = Command::new("tar")
            .args(["-xzOf"])
            .arg(&bundles[0])
            .arg("./export.md")
            .output()
            .unwrap();
        assert!(export.status.success());
        let export = String::from_utf8(export.stdout).unwrap();
        assert!(export.contains("- Status: failed\n"), "{export}");
        assert!(export.contains(VERIFY_NOT_RUN), "{export}");
        assert!(
            export.contains("Read-only violation: changed paths: base.txt."),
            "{export}"
        );
        let output = export
            .split_once("\n## Output\n")
            .unwrap()
            .1
            .split_once("\n## Diff\n")
            .unwrap()
            .0;
        assert_eq!(output, format!("{REPORT}\n"));
    }

    fn assert_one_attempt(&self, expected: &str) {
        let calls = fs::read_to_string(&self.log).unwrap();
        assert_eq!(
            calls.lines().filter(|line| line.contains("--upload")).count(),
            1,
            "{calls}"
        );
        let events = self.store.get_events(self.id.as_str()).unwrap();
        let attempts: Vec<_> = events
            .iter()
            .filter_map(|e| e.metadata.as_ref()?.get("backup")?.as_str())
            .collect();
        assert_eq!(attempts, vec![expected], "{events:?}");
    }
}

#[tokio::test]
async fn read_only_stop_preserves_declared_report_in_single_backup() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, true, false);
    f.settle().await;
    f.assert_failed_and_reviewable(home.path());
    f.assert_report();
    f.settle().await;
    f.assert_failed_and_reviewable(home.path());
    f.assert_report();
    f.assert_one_attempt("uploaded");
    assert!(f.store.backup_url(f.id.as_str()).unwrap().is_some());
}

#[tokio::test]
async fn read_only_stop_upload_failure_is_warning_only_and_not_retried() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), true, true, true);
    f.settle().await;
    f.assert_failed_and_reviewable(home.path());
    f.assert_report();
    assert_eq!(f.store.latest_error(f.id.as_str()).as_deref(), Some(VERIFY_NOT_RUN));
    f.settle().await;
    f.assert_one_attempt("failed");
    assert!(f.store.backup_url(f.id.as_str()).unwrap().is_none());
    let events = f.store.get_events(f.id.as_str()).unwrap();
    let warning = events.iter().find(|e| e.detail.starts_with("Backup failed:")).unwrap();
    assert_eq!(warning.event_kind, EventKind::Milestone);
    assert!(warning.detail.contains("synthetic upload failure"));
}

#[tokio::test]
async fn read_only_stop_missing_optional_report_without_backup_stays_failed() {
    let _permit = test_subprocess::acquire();
    let home = tempfile::tempdir().unwrap();
    let _guard = paths::AidHomeGuard::set(home.path());
    let f = Fixture::new(home.path(), false, false, false);
    f.settle().await;
    f.assert_failed_and_reviewable(home.path());
    assert!(!paths::task_dir(f.id.as_str()).join("result.md").exists());
    assert!(!f.log.exists());
    assert_eq!(fs::read_dir(&f.capture).unwrap().count(), 0);
    let task = f.store.get_task(f.id.as_str()).unwrap().unwrap();
    assert!(task.delivery_assessment.is_none());
}
