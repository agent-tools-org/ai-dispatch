// Fake-agent capacity flows through real process completion, salvage and dispatch.
// Pins single recovery, retry budget, cascade and unchanged read-only Git HEAD.

use super::*;
use crate::cmd::run::run_lifecycle::{LifecycleMode, post_run_lifecycle};
use crate::cmd::run::run_prompt::PromptBundle;
use std::{
    path::{Path, PathBuf},
    process::Command,
};

fn git(dir: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .current_dir(dir)
        .args(args)
        .output()
        .expect("git");
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    String::from_utf8(output.stdout)
        .expect("utf8")
        .trim()
        .into()
}

fn repository() -> (tempfile::TempDir, PathBuf, String) {
    let repo = tempfile::tempdir().expect("repo");
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").expect("base");
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    let worktree = crate::worktree::create_worktree(repo.path(), "fix/capacity-fixture", None)
        .expect("worktree")
        .path;
    let initial_head = git(&worktree, &["rev-parse", "HEAD"]);
    (repo, worktree, initial_head)
}

fn configured_store(row: &Task, repeated: bool) -> (Arc<Store>, RunArgs) {
    let args = RunArgs {
        agent_name: "capacity-fixture".into(),
        dir: row.worktree_path.clone(),
        repo: row.repo_path.clone(),
        worktree: row.worktree_branch.clone(),
        read_only: row.read_only,
        verify: row.verify.clone(),
        retry: if repeated { 2 } else { 0 },
        foreground: true,
        cascade: vec!["capacity-peer".into()],
        no_audit: true,
        skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()],
        ..Default::default()
    };
    let store = Arc::new(Store::open_memory().expect("store"));
    store.insert_task(&row).expect("task");
    store
        .update_task_dispatch_args(row.id.as_str(), &args.dispatch_args_json().expect("json"))
        .expect("args");
    (store, args)
}

struct Fixture {
    _home: tempfile::TempDir,
    _guard: paths::AidHomeGuard,
    _repo: tempfile::TempDir,
    worktree: PathBuf,
    store: Arc<Store>,
    row: Task,
    args: RunArgs,
    initial_head: String,
    failure: String,
}

impl Fixture {
    fn new(repeated: bool, read_only: bool) -> Self {
        let home = tempfile::tempdir().expect("home");
        let guard = paths::AidHomeGuard::set(home.path());
        paths::ensure_dirs().expect("dirs");
        let (repo, worktree, initial_head) = repository();
        let error = serde_json::json!({"type":"error", "message":CAPACITY});
        let failure = format!("echo '{error}'; exit 1");
        install_agent(
            home.path(),
            "capacity-fixture",
            if repeated { &failure } else { "echo completed" },
        );
        install_agent(home.path(), "capacity-peer", "echo peer-completed");
        let mut row = task("t-capacity-fake", &worktree);
        row.agent = AgentKind::Custom;
        row.custom_agent_name = Some("capacity-fixture".into());
        row.read_only = read_only;
        row.status = TaskStatus::Running;
        row.repo_path = Some(repo.path().display().to_string());
        row.worktree_path = Some(worktree.display().to_string());
        row.worktree_branch = Some("fix/capacity-fixture".into());
        row.start_sha = Some(initial_head.clone());
        if !read_only {
            row.verify = Some("true".into());
            row.verify_status = VerifyStatus::Pending;
        }
        let (store, args) = configured_store(&row, repeated);
        Self {
            _home: home,
            _guard: guard,
            _repo: repo,
            worktree,
            store,
            row,
            args,
            initial_head,
            failure,
        }
    }

    async fn fail_initial_agent(&self) {
        let mut command = tokio::process::Command::new("/bin/sh");
        let body = if self.row.read_only {
            self.failure.clone()
        } else {
            format!("printf partial > base.txt; {}", self.failure)
        };
        command.current_dir(&self.worktree).args(["-c", &body]);
        let agent =
            crate::agent::registry::resolve_custom_agent("capacity-fixture").expect("agent");
        crate::cmd::run::run_agent::run_agent_process(
            agent.as_ref(),
            command,
            &self.row.id,
            &self.store,
            &paths::log_path(self.row.id.as_str()),
            None,
            self.row.requested_model.as_deref(),
            agent.streaming(),
            None,
            Default::default(),
        )
        .await
        .expect("initial failing agent");
        let head = git(&self.worktree, &["rev-parse", "HEAD"]);
        if self.row.read_only {
            assert_eq!(head, self.initial_head);
        } else {
            assert_ne!(head, self.initial_head);
            assert!(
                git(&self.worktree, &["log", "-1", "--format=%s"])
                    .starts_with("wip: partial work salvage")
            );
        }
        assert!(git(&self.worktree, &["status", "--porcelain"]).is_empty());
        // An expired fixture marker keeps the suite short; recovery still backs off five seconds.
        let past = chrono::Local::now().naive_local() - chrono::Duration::minutes(1);
        std::fs::write(
            rate_limit::group_marker_path(
                &self.row.agent,
                self.row.custom_agent_name.as_deref(),
                &provider_capacity::group(self.row.requested_model.as_deref()),
            ),
            format!(
                "recovery_at: {}\nmessage: {CAPACITY}\n",
                rate_limit::format_recovery(past)
            ),
        )
        .expect("expiry");
    }

    async fn continue_task(&self) -> TaskId {
        let baseline = crate::cmd::run::run_dirty::capture_baseline(
            &self.store,
            &self.row.id,
            self.args.dir.as_deref(),
        )
        .expect("baseline");
        let prompt = PromptBundle {
            effective_prompt: "task".into(),
            context_files: vec![],
            prompt_tokens: 0,
            injected_memory_ids: vec![],
        };
        post_run_lifecycle(
            LifecycleMode::Background,
            &self.store,
            &self.row.id,
            &self.args,
            self.row.agent,
            "capacity-fixture",
            self.args.dir.as_ref(),
            self.args.repo.as_ref(),
            self.row.worktree_path.as_ref(),
            None,
            &[],
            &prompt,
            TaskStatus::Failed,
            baseline.as_deref(),
        )
        .await
        .expect("lifecycle")
        .expect("continuation")
    }

    fn assert_completion(&self, final_id: &TaskId, repeated: bool) {
        assert_eq!(
            self.store
                .get_task(final_id.as_str())
                .expect("lookup")
                .expect("final")
                .status,
            TaskStatus::Done
        );
        let chain = self
            .store
            .get_retry_chain(final_id.as_str())
            .expect("chain");
        self.assert_verification(final_id);
        assert_eq!(chain.len(), if repeated { 3 } else { 2 });
        for child in chain.iter().filter(|entry| entry.parent_task_id.is_some()) {
            let saved = RunArgs::saved_for_task(&self.store, child.id.as_str())
                .expect("saved")
                .expect("args");
            assert_eq!(saved.retry, self.args.retry);
            if child.agent_display_name() == "capacity-peer" {
                assert_eq!(saved.prompt, "Original task");
            }
        }
        assert_eq!(
            chain.last().expect("last").agent_display_name(),
            if repeated {
                "capacity-peer"
            } else {
                "capacity-fixture"
            }
        );
        if self.row.read_only {
            assert_eq!(
                git(&self.worktree, &["rev-parse", "HEAD"]),
                self.initial_head
            );
        }
    }
    fn assert_verification(&self, final_id: &TaskId) {
        if !self.row.read_only {
            assert_eq!(
                self.store
                    .get_task(self.row.id.as_str())
                    .expect("parent")
                    .expect("row")
                    .verify_status,
                VerifyStatus::Skipped
            );
            assert_eq!(
                self.store
                    .get_task(final_id.as_str())
                    .expect("child")
                    .expect("row")
                    .verify_status,
                VerifyStatus::Passed
            );
        }
    }
}

async fn flow(repeated: bool, read_only: bool) {
    let _permit = crate::test_subprocess::acquire();
    let fixture = Fixture::new(repeated, read_only);
    fixture.fail_initial_agent().await;
    let final_id = fixture.continue_task().await;
    fixture.assert_completion(&final_id, repeated);
}

#[tokio::test]
async fn provider_capacity_fake_agent_continues_once_then_succeeds_without_retry() {
    flow(false, false).await;
}

#[tokio::test]
async fn provider_capacity_second_failure_cascades_without_consuming_retry() {
    flow(true, false).await;
}

#[tokio::test]
async fn provider_capacity_read_only_continuation_does_not_commit() {
    flow(false, true).await;
}
