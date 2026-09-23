// Cross-project budget gates through the real CLI, without external agents.
// Uses isolated Git checkouts, persisted task usage and dry-run dispatch.
// Deps: common command harness, rusqlite, tempfile, Git CLI.

mod common;
use std::{path::{Path, PathBuf}, process::{Command, Output}};

struct Fixture {
    _root: tempfile::TempDir,
    home: PathBuf,
    caller: PathBuf,
    target: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = tempfile::tempdir().unwrap();
        let home = root.path().join("home");
        let caller = root.path().join("caller-checkout");
        let target = root.path().join("target-checkout");
        std::fs::create_dir(&home).unwrap();
        for (repo, id) in [(&caller, "billing-a"), (&target, "billing-b")] {
            std::fs::create_dir_all(repo.join(".aid")).unwrap();
            git(repo, &["init", "-q", "-b", "main"]);
            std::fs::write(repo.join(".gitignore"), ".aid/\n").unwrap();
            git(repo, &["add", ".gitignore"]);
            git(repo, &["-c", "user.name=Test", "-c", "user.email=test@example.invalid", "commit", "-qm", "base"]);
            std::fs::write(repo.join(".aid/project.toml"), format!("[project]\nid = '{id}'\n")).unwrap();
        }
        Self { _root: root, home, caller, target }
    }

    fn dispatch(&self, dir: Option<&Path>, id: &str) -> Output {
        let mut cmd = common::aid_cmd_with_cwd(&self.home, &self.caller);
        cmd.args(["run", "grok", "Inspect project architecture", "--dry-run", "--no-skill", "--no-hint", "--id", id]);
        if let Some(dir) = dir { cmd.arg("--dir").arg(dir); }
        cmd.output().unwrap()
    }

    fn budget(&self, name: &str, limits: &str) {
        std::fs::write(self.home.join("config.toml"), format!(
            "[[usage.budget]]\nname = '{name}'\n{limits}\n"
        )).unwrap();
    }

    fn db(&self) -> rusqlite::Connection {
        rusqlite::Connection::open(self.home.join("aid.db")).unwrap()
    }

    fn seed_usage(&self, dir: &Path, id: &str) -> String {
        success(self.dispatch(Some(dir), id));
        self.db().execute("UPDATE tasks SET status = 'done', tokens = 2000, cost_usd = 2.0 WHERE id = ?1", [id]).unwrap();
        self.db().query_row("SELECT project_id FROM tasks WHERE id = ?1", [id], |row| row.get(0)).unwrap()
    }

    fn assert_rejected(&self, output: Output, budget: &str, id: &str) {
        let error = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success(), "budget was bypassed: {}", String::from_utf8_lossy(&output.stdout));
        assert!(error.contains("Budget limit exceeded") && error.contains(budget), "{error}");
        let count: i64 = self.db().query_row("SELECT count(*) FROM tasks WHERE id = ?1", [id], |row| row.get(0)).unwrap();
        assert_eq!(count, 0, "budget rejection must precede task creation");
    }
}

fn git(repo: &Path, args: &[&str]) {
    let output = Command::new("git").arg("-C").arg(repo).args(args).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

fn success(output: Output) {
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
}

#[test]
fn exhausted_caller_budget_does_not_block_target() {
    let f = Fixture::new();
    f.budget("billing-a", "cost_limit_usd = 1.0\nexternal_cost_usd = 2.0");
    success(f.dispatch(Some(&f.target), "t-target"));
    f.assert_rejected(f.dispatch(None, "t-caller"), "billing-a", "t-caller");
}

#[test]
fn target_budget_cannot_be_bypassed_by_callers_cwd() {
    let f = Fixture::new();
    f.budget("billing-b", "cost_limit_usd = 1.0\nexternal_cost_usd = 2.0");
    f.assert_rejected(f.dispatch(Some(&f.target), "t-target"), "billing-b", "t-target");
}

#[test]
fn custom_project_id_counts_persisted_usage_for_cost_token_and_task_caps() {
    for limit in ["cost_limit_usd = 1.0", "token_limit = 1000", "task_limit = 1"] {
        let f = Fixture::new();
        assert_eq!(f.seed_usage(&f.target, "t-used"), "billing-b");
        f.budget("billing-b", limit);
        f.assert_rejected(f.dispatch(Some(&f.target), "t-blocked"), "billing-b", "t-blocked");
    }
}

#[test]
fn relative_dir_and_linked_worktree_share_the_target_budget() {
    let f = Fixture::new();
    f.seed_usage(&f.target, "t-used");
    let linked = f._root.path().join("linked");
    git(&f.target, &["worktree", "add", "--detach", linked.to_str().unwrap()]);
    std::fs::create_dir(f.target.join("src")).unwrap();
    f.budget("billing-b", "cost_limit_usd = 1.0");
    f.assert_rejected(f.dispatch(Some(Path::new("../target-checkout/src")), "t-relative"), "billing-b", "t-relative");
    f.assert_rejected(f.dispatch(Some(&linked), "t-linked"), "billing-b", "t-linked");
}

#[test]
fn unconfigured_repo_uses_its_persisted_identity_and_non_git_has_no_project_budget() {
    let f = Fixture::new();
    std::fs::remove_file(f.target.join(".aid/project.toml")).unwrap();
    let id = f.seed_usage(&f.target, "t-used");
    assert!(id.starts_with("target-checkout-"));
    f.budget(&id, "cost_limit_usd = 1.0");
    f.assert_rejected(f.dispatch(Some(&f.target), "t-blocked"), &id, "t-blocked");
    f.budget("billing-a", "cost_limit_usd = 1.0\nexternal_cost_usd = 2.0");
    success(f.dispatch(Some(&f.home), "t-non-git"));
}

#[test]
fn retry_from_another_project_rechecks_the_original_target_budget() {
    let f = Fixture::new();
    f.seed_usage(&f.target, "t-used");
    f.budget("billing-b", "cost_limit_usd = 1.0");
    let output = common::aid_cmd_with_cwd(&f.home, &f.caller)
        .args(["retry", "t-used", "--feedback", "Try again"]).output().unwrap();
    f.assert_rejected(output, "billing-b", "unused-child-id");
    let count: i64 = f.db().query_row("SELECT count(*) FROM tasks", [], |row| row.get(0)).unwrap();
    assert_eq!(count, 1, "retry must not create a child before the budget gate");
}

#[test]
fn batch_resolves_each_tasks_budget_from_its_declared_directory() {
    let f = Fixture::new();
    f.seed_usage(&f.target, "t-used");
    f.budget("billing-b", "cost_limit_usd = 1.0");
    let batch = f._root.path().join("budget.toml");
    std::fs::write(&batch, "[[tasks]]\nagent = 'grok'\nprompt = 'Inspect architecture'\ndir = 'target-checkout'\nid = 't-batch-budget'\nno_skill = true\n").unwrap();
    let output = common::aid_cmd_with_cwd(&f.home, &f.caller)
        .arg("batch").arg(&batch).args(["--dry-run", "--no-prompt"]).output().unwrap();
    f.assert_rejected(output, "billing-b", "t-batch-budget");
}

#[test]
fn near_limit_warning_uses_target_usage_only() {
    let f = Fixture::new();
    f.seed_usage(&f.target, "t-used");
    f.budget("billing-b", "cost_limit_usd = 2.2");
    let output = f.dispatch(Some(&f.target), "t-near");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let text = format!("{}{}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
    assert!(text.contains("Budget 'billing-b': cost usage at"), "{text}");
    assert!(text.contains("Auto-enabling budget mode"), "{text}");
    let caller = f.dispatch(None, "t-caller-not-near");
    assert!(caller.status.success());
    assert!(!String::from_utf8_lossy(&caller.stderr).contains("Auto-enabling budget mode"));
}

#[test]
fn project_budget_usage_agrees_with_gate_and_does_not_guess_legacy_or_other_project_ownership() {
    let f = Fixture::new();
    for id in ["t-own", "t-other", "t-legacy", "t-old"] {
        f.seed_usage(&f.target, id);
    }
    let db = f.db();
    db.execute("UPDATE tasks SET cost_usd = 0.5, tokens = 500 WHERE id = 't-own'", []).unwrap();
    db.execute("UPDATE tasks SET project_id = 'different-project' WHERE id = 't-other'", []).unwrap();
    db.execute("UPDATE tasks SET project_id = NULL WHERE id = 't-legacy'", []).unwrap();
    db.execute("UPDATE tasks SET created_at = '2000-01-01T00:00:00Z' WHERE id = 't-old'", []).unwrap();
    f.budget("billing-b", "window = 'daily'\ncost_limit_usd = 1.0\nexternal_cost_usd = 0.25\nexternal_tokens = 250\nexternal_tasks = 1");
    let output = common::aid_cmd_in(&f.home)
        .args(["usage", "--json"]).output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    let budget = &report["snapshot"]["budget_rows"][0];
    assert_eq!(budget["cost_usd"], 0.75);
    assert_eq!(budget["tokens"], 750);
    assert_eq!(budget["tasks"], 2);
    success(f.dispatch(Some(&f.target), "t-under-cap"));
}

#[test]
fn agent_budget_is_still_enforced_without_a_target_project() {
    let f = Fixture::new();
    f.budget("grok-quota", "agent = 'grok'\ncost_limit_usd = 1.0\nexternal_cost_usd = 2.0");
    f.assert_rejected(f.dispatch(Some(&f.home), "t-agent-cap"), "grok-quota", "t-agent-cap");
}
