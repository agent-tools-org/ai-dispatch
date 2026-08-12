// Tests for stable project identity resolution and unattributed filtering.
// Exports: none; loaded by identity.rs under #[cfg(test)].
// Deps: super, tempfile, std::process::Command.

use super::*;
use std::fs;
use std::process::Command;
use tempfile::TempDir;

fn git(repo: &Path, args: &[&str]) {
    assert!(
        Command::new("git")
            .args(["-C", &repo.to_string_lossy()])
            .args(args)
            .status()
            .unwrap()
            .success()
    );
}

fn init_repo(repo: &Path) {
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test"]);
    fs::write(repo.join("README"), "x\n").unwrap();
    git(repo, &["add", "README"]);
    git(repo, &["commit", "-m", "init"]);
}

fn write_project_toml(repo: &Path, id: &str) {
    let aid = repo.join(".aid");
    fs::create_dir_all(&aid).unwrap();
    fs::write(
        aid.join("project.toml"),
        format!("[project]\nid = \"{id}\"\n"),
    )
    .unwrap();
}

#[test]
fn resolve_uses_project_toml_id() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("app");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    write_project_toml(&repo, "my-app");
    assert_eq!(resolve_project_id(&repo).as_deref(), Some("my-app"));
}

#[test]
fn main_and_linked_worktree_share_toml_id() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("shared");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    write_project_toml(&repo, "shared-proj");
    let wt = tmp.path().join("wt-feature");
    git(
        &repo,
        &[
            "worktree",
            "add",
            &wt.to_string_lossy(),
            "-b",
            "feature/shared",
        ],
    );
    assert_eq!(resolve_project_id(&repo).as_deref(), Some("shared-proj"));
    assert_eq!(resolve_project_id(&wt).as_deref(), Some("shared-proj"));
}

#[test]
fn outside_git_is_unattributed_none() {
    let tmp = TempDir::new().unwrap();
    let plain = tmp.path().join("no-git");
    fs::create_dir_all(&plain).unwrap();
    assert_eq!(resolve_project_id(&plain), None);
    assert_eq!(project_display(None), UNATTRIBUTED);
}

#[test]
fn git_without_toml_uses_path_based_id_shared_by_worktree() {
    let tmp = TempDir::new().unwrap();
    let repo = tmp.path().join("bareish");
    fs::create_dir_all(&repo).unwrap();
    init_repo(&repo);
    let expected = path_based_project_id(&repo.canonicalize().unwrap());
    assert_eq!(resolve_project_id(&repo).as_deref(), Some(expected.as_str()));
    let wt = tmp.path().join("wt-path");
    git(
        &repo,
        &["worktree", "add", &wt.to_string_lossy(), "-b", "feat/path"],
    );
    assert_eq!(resolve_project_id(&wt).as_deref(), Some(expected.as_str()));
}

#[test]
fn retain_project_keeps_unattributed_bucket_explicit() {
    use crate::types::{AgentKind, Task, TaskId, TaskStatus, VerifyStatus};
    use chrono::Local;

    fn task(id: &str, project_id: Option<&str>) -> Task {
        Task {
            id: TaskId(id.to_string()),
            agent: AgentKind::Codex,
            custom_agent_name: None,
            prompt: "p".into(),
            resolved_prompt: None,
            category: None,
            status: TaskStatus::Done,
            parent_task_id: None,
            workgroup_id: None,
            caller_kind: None,
            caller_session_id: None,
            agent_session_id: None,
            repo_path: None,
            project_id: project_id.map(str::to_string),
            worktree_path: None,
            worktree_branch: None,
            final_head_sha: None,
            final_branch: None,
            start_sha: None,
            log_path: None,
            output_path: None,
            tokens: None,
            prompt_tokens: None,
            duration_ms: None,
            requested_model: None,
            observed_model: None,
            attribution_source: None,
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

    let mut tasks = vec![
        task("t-a", Some("ai-dispatch")),
        task("t-u", None),
        task("t-b", Some("other")),
    ];
    retain_project(&mut tasks, Some("ai-dispatch"));
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id.as_str(), "t-a");

    let mut tasks = vec![
        task("t-a", Some("ai-dispatch")),
        task("t-u", None),
        task("t-b", Some("other")),
    ];
    retain_project(&mut tasks, None);
    assert_eq!(tasks.len(), 1);
    assert_eq!(tasks[0].id.as_str(), "t-u");
    assert!(tasks[0].project_id.is_none());
}

#[test]
fn project_filter_banner_marks_active_filter() {
    let current = project_filter_banner(Some("ai-dispatch"), false);
    assert!(current.contains("project:ai-dispatch"));
    assert!(current.contains("--all"));
    let all = project_filter_banner(Some("ai-dispatch"), true);
    assert!(all.contains("project:*"));
    let unattr = project_filter_banner(None, false);
    assert!(unattr.contains(UNATTRIBUTED));
}

#[test]
fn default_filter_does_not_silently_drop_without_banner_contract() {
    // Literal contract: whenever the default filter is active, the banner must
    // name the filter and the escape hatch. Callers must print this — silent
    // drops are the failure mode this feature exists to prevent.
    let banner = project_filter_banner(Some("proj-x"), false);
    assert_eq!(
        banner,
        "project:proj-x (use --all to show every project)"
    );
    let unattr = project_filter_banner(None, false);
    assert_eq!(
        unattr,
        "project:unattributed (use --all to show every project)"
    );
}
