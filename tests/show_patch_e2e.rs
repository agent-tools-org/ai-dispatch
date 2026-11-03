// CLI regressions for complete, raw delivery patches over real Git checkouts.
// Covers retries, dirty state, root-relative paths, config isolation and failures.

mod common;
#[path = "common/show_patch.rs"]
mod fixture;
use fixture::{Fixture, commit, git};
use std::fs;

fn contains(patch: &[u8], text: &str) -> bool {
    patch.windows(text.len()).any(|window| window == text.as_bytes())
}

#[test]
fn worktree_retry_includes_first_attempt_commits() {
    let f = Fixture::new();
    let wt = f.worktree();
    f.task("t-first", &wt, true, None, Some(&f.base));
    fs::write(wt.join("first.txt"), "first attempt\n").unwrap();
    let first = commit(&wt, "first attempt");
    f.task("t-retry", &wt, true, Some("t-first"), Some(&first));
    fs::write(wt.join("second.txt"), "second attempt\n").unwrap();
    commit(&wt, "second attempt");
    let patch = f.bytes("t-retry");
    assert!(patch.starts_with(b"diff --git "));
    assert!(contains(&patch, "b/first.txt"));
    assert!(contains(&patch, "b/second.txt"));
}

#[test]
fn inplace_retry_uses_earliest_start_sha() {
    let f = Fixture::new();
    f.task("t-first", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("first.txt"), "first attempt\n").unwrap();
    let first = commit(&f.repo, "first attempt");
    f.task("t-retry", &f.repo, false, Some("t-first"), Some(&first));
    fs::write(f.repo.join("second.txt"), "second attempt\n").unwrap();
    commit(&f.repo, "second attempt");
    let patch = f.bytes("t-retry");
    assert!(contains(&patch, "b/first.txt"));
    assert!(contains(&patch, "b/second.txt"));
}

#[test]
fn subdirectory_task_includes_changes_elsewhere_with_root_relative_paths() {
    let f = Fixture::new();
    let subdir = f.repo.join("subdir");
    fs::create_dir(&subdir).unwrap();
    f.task("t-subdir", &subdir, false, None, Some(&f.base));
    fs::write(f.repo.join("sibling.txt"), "tracked outside task dir\n").unwrap();
    fs::write(f.repo.join("untracked.txt"), "untracked outside task dir\n").unwrap();
    let patch = f.bytes("t-subdir");
    assert!(contains(&patch, "diff --git a/sibling.txt b/sibling.txt"));
    assert!(contains(&patch, "diff --git a/untracked.txt b/untracked.txt"));
}

#[test]
fn patch_ignores_user_diff_config() {
    let f = Fixture::new();
    f.task("t-config", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("base.txt"), "new tracked content\n").unwrap();
    fs::write(f.repo.join("new.txt"), "new untracked content\n").unwrap();
    let expected = f.bytes("t-config");
    for (key, value) in [("diff.noprefix", "true"), ("diff.mnemonicPrefix", "true"),
        ("diff.relative", "true"), ("diff.external", "false")]
    {
        git(&f.repo, &["config", key, value]);
    }
    let actual = f.bytes("t-config");
    assert_eq!(actual, expected);
    assert!(contains(&actual, "diff --git a/base.txt b/base.txt"));
    assert!(contains(&actual, "diff --git a/new.txt b/new.txt"));
}

#[test]
fn patch_contains_every_delivery_state_without_changing_index() {
    let f = Fixture::new();
    f.task("t-states", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("committed.txt"), "committed\n").unwrap();
    commit(&f.repo, "committed delivery");
    fs::write(f.repo.join("base.txt"), "staged\n").unwrap();
    git(&f.repo, &["add", "base.txt"]);
    fs::write(f.repo.join("sibling.txt"), "unstaged\n").unwrap();
    fs::write(f.repo.join("new.txt"), "untracked\n").unwrap();
    fs::write(f.repo.join("Cargo.lock"), "lockfile delivery\n").unwrap();
    let before = git(&f.repo, &["diff", "--cached"]);
    let patch = f.bytes("t-states");
    assert!(patch.starts_with(b"diff --git "));
    for file in ["committed.txt", "base.txt", "sibling.txt", "new.txt", "Cargo.lock"] {
        assert!(contains(&patch, &format!("b/{file}")), "missing {file}");
    }
    for content in ["+committed", "+staged", "+unstaged", "+untracked", "+lockfile delivery"] {
        assert!(contains(&patch, content));
    }
    assert_eq!(git(&f.repo, &["diff", "--cached"]), before);
}

#[test]
fn patch_excludes_untracked_aic_reports() {
    let f = Fixture::new();
    f.task("t-report", &f.repo, false, None, Some(&f.base));
    fs::create_dir_all(f.repo.join(".aic/audits/x")).unwrap();
    fs::write(f.repo.join(".aic/audits/x/report.md"), "audit output\n").unwrap();
    fs::create_dir_all(f.repo.join("nested/.aic/audits/x")).unwrap();
    fs::write(f.repo.join("nested/.aic/audits/x/report.md"), "audit output\n").unwrap();
    fs::write(f.repo.join("delivery.txt"), "actual delivery\n").unwrap();
    let patch = f.bytes("t-report");
    assert!(!contains(&patch, ".aic/"));
    assert!(contains(&patch, "b/delivery.txt"));
}

#[test]
fn patch_includes_commits_after_task_completion() {
    let f = Fixture::new();
    f.task("t-finished", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("later.txt"), "after completion\n").unwrap();
    commit(&f.repo, "later commit");
    assert!(contains(&f.bytes("t-finished"), "b/later.txt"));
}

#[test]
fn missing_worktree_and_missing_base_fail_without_stdout() {
    let f = Fixture::new();
    let missing = f.repo.join("missing-worktree");
    f.task("t-missing-wt", &missing, true, None, Some(&f.base));
    f.task("t-missing-base", &f.repo, false, None, None);
    f.task("t-invalid-base", &f.repo, false, None, Some("invalid-base"));
    for (id, diagnostic) in [("t-missing-wt", "checkout"), ("t-missing-base", "base"),
        ("t-invalid-base", "base"), ("t-unknown", "not found")]
    {
        let output = f.patch(id);
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        assert!(String::from_utf8_lossy(&output.stderr).contains(diagnostic));
    }
}

#[test]
fn non_git_checkout_fails_without_stdout() {
    let f = Fixture::new();
    f.task("t-not-git", f.home.path(), false, None, Some(&f.base));
    let output = f.patch("t-not-git");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("repository"));
}

#[test]
fn no_change_is_empty_stdout_and_success() {
    let f = Fixture::new();
    f.task("t-empty", &f.repo, false, None, Some(&f.base));
    assert!(f.bytes("t-empty").is_empty());
}

#[test]
fn patch_preserves_non_utf8_bytes() {
    let f = Fixture::new();
    f.task("t-bytes", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("base.txt"), b"tracked \xff\n").unwrap();
    fs::write(f.repo.join("new.txt"), b"untracked \xfe\n").unwrap();
    let patch = f.bytes("t-bytes");
    assert!(patch.contains(&0xff));
    assert!(patch.contains(&0xfe));
    assert!(!patch.windows(3).any(|window| window == b"\xef\xbf\xbd"));
}

#[test]
fn worktree_without_resolvable_base_ref_uses_retry_chain() {
    let f = Fixture::new();
    let wt = f.worktree();
    git(&f.repo, &["branch", "-m", "unusual-base"]);
    f.task("t-first", &wt, true, None, Some(&f.base));
    fs::write(wt.join("first.txt"), "first attempt\n").unwrap();
    let first = commit(&wt, "first attempt");
    f.task("t-retry", &wt, true, Some("t-first"), Some(&first));
    assert!(contains(&f.bytes("t-retry"), "b/first.txt"));
}

#[test]
fn retry_chain_stops_at_an_ancestor_start_this_repository_cannot_name() {
    let f = Fixture::new();
    f.task("t-parent", &f.repo, false, None, Some("unresolvable-parent-base"));
    let wt = f.worktree();
    f.task("t-child", &wt, false, Some("t-parent"), Some(&f.base));
    fs::write(wt.join("child.txt"), "child checkout\n").unwrap();
    assert!(contains(&f.bytes("t-child"), "b/child.txt"));
}

#[test]
fn worktree_patch_uses_branch_merge_base_even_if_task_started_later() {
    let f = Fixture::new();
    let wt = f.worktree();
    fs::write(wt.join("earlier.txt"), "earlier branch delivery\n").unwrap();
    let start = commit(&wt, "earlier branch commit");
    f.task("t-later-start", &wt, true, None, Some(&start));
    assert!(contains(&f.bytes("t-later-start"), "b/earlier.txt"));
}

#[test]
fn stdout_is_exactly_the_raw_patch_without_headers_or_notes() {
    let f = Fixture::new();
    f.task("t-raw", &f.repo, false, None, Some(&f.base));
    fs::write(f.repo.join("base.txt"), "delivered change\n").unwrap();
    let expected = git(&f.repo, &[
        "-c", "diff.noprefix=false", "-c", "diff.mnemonicPrefix=false",
        "-c", "diff.relative=false", "-c", "core.quotePath=true", "diff",
        "--no-ext-diff", "--no-color", "--no-textconv", "--no-relative",
        "--src-prefix=a/", "--dst-prefix=b/", "--submodule=diff", &f.base, "--",
    ]);
    assert!(expected.starts_with(b"diff --git "));
    assert_eq!(f.bytes("t-raw"), expected);
}
