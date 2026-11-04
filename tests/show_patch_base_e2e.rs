// CLI coverage for the resolved patch baseline consumed by cross-auditors.
// Uses persisted tasks and real repositories, including dirty configuration.

mod common;
#[path = "common/show_patch.rs"]
mod fixture;
use fixture::{Fixture, commit, git};
use std::fs;

fn assert_base(f: &Fixture, id: &str) {
    let output = common::aid_cmd_in(f.home.path())
        .args(["show", id, "--patch-base"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(output.stdout, format!("{}\n", f.base).as_bytes());
    assert_eq!(
        git(
            &f.repo,
            &["rev-parse", "--verify", &format!("{}^{{commit}}", f.base)]
        ),
        output.stdout
    );
}

#[test]
fn inplace_base_ignores_changed_working_tree_configuration_and_head() {
    let f = Fixture::new();
    f.task("t-inplace", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("base.txt"), "delivery\n").unwrap();
    commit(&f.repo, "delivery");
    fs::create_dir(f.repo.join(".aic")).unwrap();
    fs::write(f.repo.join(".aic/config.toml"), "invalid configuration").unwrap();
    fs::write(f.repo.join("main"), "fake base").unwrap();
    assert_base(&f, "t-inplace");
    assert!(!f.bytes("t-inplace").is_empty());
}

#[test]
fn worktree_base_includes_work_before_task_start() {
    let f = Fixture::new();
    let wt = f.worktree();
    fs::write(wt.join("earlier.txt"), "earlier work\n").unwrap();
    let start = commit(&wt, "earlier work");
    f.task("t-worktree", &wt, true, None, Some(&start));
    assert_base(&f, "t-worktree");
    assert!(!f.bytes("t-worktree").is_empty());
}

#[test]
fn retry_chain_base_folds_all_recorded_starts() {
    for worktree in [false, true] {
        let f = Fixture::new();
        let root = if worktree { f.worktree() } else { f.repo.clone() };
        f.task("t-first", &root, worktree, None, Some(&f.base));
        fs::write(root.join("first.txt"), "first attempt\n").unwrap();
        let first = commit(&root, "first attempt");
        f.task("t-retry", &root, worktree, Some("t-first"), Some(&first));
        assert_base(&f, "t-retry");
        assert!(!f.bytes("t-retry").is_empty());
    }
}

#[test]
fn no_recorded_start_refuses_identically_to_patch() {
    let f = Fixture::new();
    f.task("t-no-start", &f.repo, false, None, None);
    let base = common::aid_cmd_in(f.home.path())
        .args(["show", "t-no-start", "--patch-base"])
        .output()
        .unwrap();
    let patch = f.patch("t-no-start");
    assert!(!base.status.success());
    assert!(!patch.status.success());
    assert!(base.stdout.is_empty());
    assert_eq!(base.stderr, patch.stderr);
    assert!(String::from_utf8_lossy(&base.stderr).contains("retry chain has no start_sha"));
}

#[test]
fn unresolvable_recorded_start_refuses_without_fallback() {
    let f = Fixture::new();
    f.task("t-bad-start", &f.repo, false, None, Some("deadbeef"));
    let base = common::aid_cmd_in(f.home.path())
        .args(["show", "t-bad-start", "--patch-base"])
        .output()
        .unwrap();
    let patch = f.patch("t-bad-start");
    assert!(!base.status.success());
    assert!(base.stdout.is_empty());
    assert_eq!(base.stderr, patch.stderr);
    assert!(String::from_utf8_lossy(&base.stderr).contains("Cannot resolve recorded task patch base"));
}
