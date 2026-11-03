// Audit regressions for conservative patch baselines and Git isolation.
// Uses real repositories and persisted task fixtures through the compiled CLI.

mod common;
#[path = "common/show_patch.rs"]
mod fixture;
use fixture::{Fixture, commit, git};
use std::fs;

fn contains(patch: &[u8], text: &str) -> bool {
    patch.windows(text.len()).any(|window| window == text.as_bytes())
}

#[test]
fn merged_task_branches_still_show_committed_delivery() {
    for fast_forward in [true, false] {
        let f = Fixture::new();
        let wt = f.worktree();
        f.task("t-merged", &wt, true, None, Some(&f.base));
        fs::write(wt.join("delivery.txt"), "task delivery\n").unwrap();
        commit(&wt, "task delivery");
        if fast_forward {
            git(&f.repo, &["merge", "--ff-only", "delivery"]);
        } else {
            fs::write(f.repo.join("main.txt"), "main development\n").unwrap();
            commit(&f.repo, "main development");
            git(&f.repo, &["merge", "--no-ff", "-m", "merge task", "delivery"]);
        }
        assert!(contains(&f.bytes("t-merged"), "b/delivery.txt"));
    }
}

#[test]
fn reset_retry_folds_starts_instead_of_using_parent_snapshot() {
    let f = Fixture::new();
    fs::write(f.repo.join("recreated.txt"), "same content\n").unwrap();
    let parent_start = commit(&f.repo, "parent snapshot");
    f.task("t-parent", &f.repo, false, None, Some(&parent_start));
    git(&f.repo, &["reset", "--hard", &f.base]);
    f.task("t-reset", &f.repo, false, Some("t-parent"), Some(&f.base));
    fs::write(f.repo.join("recreated.txt"), "same content\n").unwrap();
    commit(&f.repo, "child recreates file");
    assert!(contains(&f.bytes("t-reset"), "b/recreated.txt"));
}

#[test]
fn unrelated_recorded_starts_fail_instead_of_omitting_changes() {
    let f = Fixture::new();
    git(&f.repo, &["checkout", "--orphan", "unrelated"]);
    git(&f.repo, &["rm", "-rf", "."]);
    fs::write(f.repo.join("other.txt"), "other history\n").unwrap();
    let unrelated = commit(&f.repo, "unrelated root");
    f.task("t-parent", &f.repo, false, None, Some(&unrelated));
    git(&f.repo, &["checkout", "main"]);
    f.task("t-child", &f.repo, false, Some("t-parent"), Some(&f.base));
    let output = f.patch("t-child");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("common ancestor"));
}

#[test]
fn missing_start_requires_a_branch_base_strictly_before_head() {
    let f = Fixture::new();
    let wt = f.worktree();
    f.task("t-no-start", &wt, true, None, None);
    fs::write(wt.join("delivery.txt"), "task delivery\n").unwrap();
    commit(&wt, "task delivery");
    assert!(contains(&f.bytes("t-no-start"), "b/delivery.txt"));
    git(&f.repo, &["merge", "--ff-only", "delivery"]);
    let output = f.patch("t-no-start");
    assert!(!output.status.success());
    assert!(output.stdout.is_empty());
    assert!(String::from_utf8_lossy(&output.stderr).contains("No safe base"));
}

#[test]
fn auditor_configuration_is_included_but_nested_audit_reports_are_excluded() {
    let f = Fixture::new();
    f.task("t-aic", &f.repo, false, None, Some(&f.base));
    for dir in [".aic/rules", "nested/.aic/audits/x"] {
        fs::create_dir_all(f.repo.join(dir)).unwrap();
    }
    fs::write(f.repo.join(".aic/config.toml"), "enabled = false\n").unwrap();
    fs::write(f.repo.join(".aic/audits"), "a file, not an audit directory\n").unwrap();
    fs::write(f.repo.join(".aic/rules/check.md"), "audit rule\n").unwrap();
    fs::write(f.repo.join("nested/.aic/audits/x/report.md"), "report\n").unwrap();
    let patch = f.bytes("t-aic");
    assert!(contains(&patch, "b/.aic/config.toml"));
    assert!(contains(&patch, "diff --git a/.aic/audits b/.aic/audits\n"));
    assert!(contains(&patch, "b/.aic/rules/check.md"));
    assert!(!contains(&patch, "nested/.aic/audits/"));
}

#[test]
fn personal_ignore_rules_do_not_hide_delivery() {
    let f = Fixture::new();
    f.task("t-ignore", &f.repo, false, None, Some(&f.base));
    let personal = f.home.path().join("personal-ignore");
    fs::write(&personal, "hidden.txt\n").unwrap();
    git(&f.repo, &["config", "core.excludesFile", personal.to_str().unwrap()]);
    fs::write(f.repo.join(".git/info/exclude"), "local-hidden.txt\n").unwrap();
    fs::write(f.repo.join(".gitignore"), "repo-ignored.txt\n").unwrap();
    for file in ["hidden.txt", "local-hidden.txt", "repo-ignored.txt"] {
        fs::write(f.repo.join(file), "delivery\n").unwrap();
    }
    let patch = f.bytes("t-ignore");
    assert!(contains(&patch, "b/hidden.txt"));
    assert!(contains(&patch, "b/local-hidden.txt"));
    assert!(!contains(&patch, "b/repo-ignored.txt"));
}

#[test]
fn submodule_ignore_configuration_cannot_hide_changes() {
    let f = Fixture::new();
    git(&f.repo, &["-c", "protocol.file.allow=always", "submodule", "add", "-q",
        f.repo.to_str().unwrap(), "module"]);
    let start = commit(&f.repo, "add submodule");
    f.task("t-module", &f.repo, false, None, Some(&start));
    git(&f.repo, &["config", "diff.ignoreSubmodules", "all"]);
    fs::write(f.repo.join("module/base.txt"), "submodule delivery\n").unwrap();
    assert!(contains(&f.bytes("t-module"), "+submodule delivery"));
}

#[test]
fn inherited_git_environment_cannot_redirect_patch_or_base_queries() {
    let f = Fixture::new();
    let other = Fixture::new();
    let wt = f.worktree();
    f.task("t-env", &wt, true, None, Some(&f.base));
    fs::write(wt.join("delivery.txt"), "correct checkout\n").unwrap();
    commit(&wt, "task delivery");
    let expected = f.bytes("t-env");
    let output = common::aid_cmd_in(f.home.path()).args(["show", "t-env", "--patch"])
        .env("GIT_DIR", other.repo.join(".git"))
        .env("GIT_WORK_TREE", &other.repo)
        .env("GIT_INDEX_FILE", other.repo.join(".git/index"))
        .env("GIT_CONFIG_PARAMETERS", format!("'core.worktree'='{}'", other.repo.display()))
        .env("GIT_CONFIG_COUNT", "1").env("GIT_CONFIG_KEY_0", "core.worktree")
        .env("GIT_CONFIG_VALUE_0", &other.repo).env("GIT_PREFIX", "elsewhere/")
        .output().unwrap();
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert_eq!(output.stdout, expected);
}

#[test]
fn missing_ancestor_rows_and_checkouts_stop_the_retry_walk() {
    for removed_checkout in [false, true] {
        let f = Fixture::new();
        if removed_checkout {
            f.task("t-parent", &f.home.path().join("removed"), true, None, Some("unavailable"));
        }
        f.task("t-child", &f.repo, false, Some("t-parent"), Some(&f.base));
        fs::write(f.repo.join("delivery.txt"), "child delivery\n").unwrap();
        assert!(contains(&f.bytes("t-child"), "b/delivery.txt"));
    }
}

#[test]
fn untracked_nested_repository_warns_without_failing_the_patch() {
    let f = Fixture::new();
    f.task("t-nested", &f.repo, false, None, Some(&f.base));
    let nested = f.repo.join("nested-repository");
    fs::create_dir(&nested).unwrap();
    git(&nested, &["init", "-q"]);
    fs::write(nested.join("content.txt"), "nested content\n").unwrap();
    let listed = git(&f.repo, &["ls-files", "--others", "--exclude-per-directory=.gitignore", "-z"]);
    assert!(listed.split(|byte| *byte == 0).any(|file| file == b"nested-repository/"));
    let raw = std::process::Command::new("git").arg("-C").arg(&f.repo)
        .args(["diff", "--no-index", "--", "/dev/null", "nested-repository/"])
        .output().unwrap();
    assert_eq!(raw.status.code(), Some(1));
    assert!(raw.stdout.is_empty());
    assert!(!raw.stderr.is_empty());
    fs::write(f.repo.join("delivery.txt"), "representable delivery\n").unwrap();
    let output = f.patch("t-nested");
    assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
    assert!(contains(&output.stdout, "b/delivery.txt"));
    assert!(!contains(&output.stdout, "nested-repository"));
    let diagnostic = String::from_utf8_lossy(&output.stderr);
    assert!(diagnostic.contains("untracked directory") && diagnostic.contains("nested-repository"));
}

#[test]
fn untracked_directory_symlink_is_represented_as_a_patch() {
    let f = Fixture::new();
    f.task("t-link", &f.repo, false, None, Some(&f.base));
    std::os::unix::fs::symlink(f.home.path(), f.repo.join("directory-link")).unwrap();
    let patch = f.bytes("t-link");
    assert!(contains(&patch, "b/directory-link"));
    assert!(contains(&patch, "new file mode 120000"));
}
