// CLI coverage for the immutable first-attempt start consumed by cross-auditors.
// Uses persisted retry chains and real Git resets, including missing history.

mod common;
#[path = "common/show_patch.rs"]
mod fixture;
use fixture::{Fixture, commit, git};
use std::fs;

fn assert_start(f: &Fixture, id: &str, start: &str) {
    let output = common::aid_cmd_in(f.home.path())
        .args(["show", id, "--start-commit"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, format!("{start}\n").as_bytes());
    assert_eq!(
        git(
            &f.repo,
            &["rev-parse", "--verify", &format!("{start}^{{commit}}")]
        ),
        output.stdout
    );
}

#[test]
fn inplace_start_ignores_changed_working_tree_configuration_and_head() {
    let f = Fixture::new();
    f.task("t-inplace", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("base.txt"), "delivery\n").unwrap();
    commit(&f.repo, "delivery");
    fs::create_dir(f.repo.join(".aic")).unwrap();
    fs::write(f.repo.join(".aic/config.toml"), "invalid configuration").unwrap();
    fs::write(f.repo.join("main"), "fake base").unwrap();
    assert_start(&f, "t-inplace", &f.base);
    assert!(!f.bytes("t-inplace").is_empty());
}

#[test]
fn worktree_start_survives_agent_reset_to_older_commit() {
    let f = Fixture::new();
    let wt = f.worktree();
    fs::create_dir(wt.join(".aic")).unwrap();
    fs::write(wt.join(".aic/config.toml"), "required policy\n").unwrap();
    let start = commit(&wt, "audit configuration");
    f.task("t-worktree", &wt, true, None, Some(&start));
    git(&wt, &["reset", "--hard", &f.base]);
    assert!(!wt.join(".aic/config.toml").exists());
    assert_start(&f, "t-worktree", &start);
}

#[test]
fn retry_chain_start_survives_first_attempt_reset_to_older_commit() {
    for worktree in [false, true] {
        let f = Fixture::new();
        let root = if worktree { f.worktree() } else { f.repo.clone() };
        fs::create_dir(root.join(".aic")).unwrap();
        fs::write(root.join(".aic/config.toml"), "required policy\n").unwrap();
        let start = commit(&root, "audit configuration");
        f.task("t-first", &root, worktree, None, Some(&start));
        git(&root, &["reset", "--hard", &f.base]);
        f.task("t-retry", &root, worktree, Some("t-first"), Some(&f.base));
        f.task("t-last", &root, worktree, Some("t-retry"), Some(&f.base));
        assert_start(&f, "t-last", &start);
    }
}

#[test]
fn first_attempt_without_recorded_start_refuses_even_when_retry_has_start() {
    let f = Fixture::new();
    f.task("t-no-start", &f.repo, false, None, None);
    f.task("t-retry", &f.repo, false, Some("t-no-start"), Some(&f.base));
    for id in ["t-no-start", "t-retry"] {
        assert_refused(&f, id, "First attempt t-no-start has no recorded start_sha");
    }
}

#[test]
fn unresolvable_recorded_start_refuses_without_fallback() {
    let f = Fixture::new();
    f.task("t-bad-start", &f.repo, false, None, Some("deadbeef"));
    f.task("t-retry", &f.repo, false, Some("t-bad-start"), Some(&f.base));
    for id in ["t-bad-start", "t-retry"] {
        assert_refused(&f, id, "Cannot resolve first attempt t-bad-start start commit");
    }
}

fn assert_refused(f: &Fixture, id: &str, message: &str) {
    let output = common::aid_cmd_in(f.home.path())
        .args(["show", id, "--start-commit"])
        .output()
        .unwrap();
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains(message), "{stderr}");
}

#[test]
fn missing_ancestor_refuses_instead_of_using_a_later_start() {
    let f = Fixture::new();
    f.task("t-middle", &f.repo, false, Some("t-missing"), Some(&f.base));
    f.task("t-last", &f.repo, false, Some("t-middle"), Some(&f.base));
    assert_refused(
        &f,
        "t-last",
        "Cannot determine start commit: missing ancestor task t-missing",
    );
}

#[test]
fn retry_cycle_refuses() {
    let f = Fixture::new();
    f.task("t-first", &f.repo, false, Some("t-last"), Some(&f.base));
    f.task("t-last", &f.repo, false, Some("t-first"), Some(&f.base));
    assert_refused(&f, "t-last", "Cycle in task retry chain");
}
