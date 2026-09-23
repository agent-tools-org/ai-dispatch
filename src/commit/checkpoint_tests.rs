// Real Git regression cases for non-mutating shared checkout recovery.
// Covers index/workfile isolation, unborn/tagged HEAD, retries and failures.
// Deps: checkpoint helper, tempfile, Git CLI.

use super::preserve_shared_checkout;
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git").arg("-C").arg(repo).args(args).output().unwrap();
    assert!(output.status.success(), "{args:?}: {}", String::from_utf8_lossy(&output.stderr));
    String::from_utf8(output.stdout).unwrap().trim().to_string()
}

fn repo(commit: bool) -> tempfile::TempDir {
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.name", "Test"]);
    git(repo.path(), &["config", "user.email", "test@example.invalid"]);
    if commit {
        std::fs::write(repo.path().join("source.txt"), "base\n").unwrap();
        git(repo.path(), &["add", "."]);
        git(repo.path(), &["commit", "-m", "base"]);
    }
    repo
}

#[test]
fn shared_checkpoint_preserves_head_index_and_working_files() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(true);
    let root = repo.path();
    let head = git(root, &["rev-parse", "HEAD"]);
    std::fs::write(root.join("source.txt"), "user staged\n").unwrap();
    git(root, &["add", "source.txt"]);
    std::fs::write(root.join("source.txt"), "user unstaged\n").unwrap();
    std::fs::write(root.join("new ü file.txt"), "task output\n").unwrap();
    let index = std::fs::read(root.join(".git/index")).unwrap();

    let saved = preserve_shared_checkout(root, "t-shared").unwrap().unwrap();

    assert_eq!(git(root, &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read(root.join(".git/index")).unwrap(), index);
    assert_eq!(git(root, &["show", ":source.txt"]), "user staged");
    assert_eq!(std::fs::read_to_string(root.join("source.txt")).unwrap(), "user unstaged\n");
    assert_eq!(git(root, &["show", &format!("{}:source.txt", saved.reference)]), "user unstaged");
    assert_eq!(git(root, &["show", &format!("{}:new ü file.txt", saved.reference)]), "task output");
    assert_eq!(git(root, &["rev-parse", &format!("{}^", saved.reference)]), head);
}

#[test]
fn unborn_shared_checkout_keeps_head_unborn_and_index_absent() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(false);
    std::fs::write(repo.path().join("first.txt"), "first\n").unwrap();
    let saved = preserve_shared_checkout(repo.path(), "t-unborn").unwrap().unwrap();
    assert!(!repo.path().join(".git/index").exists());
    assert!(!repo.path().join(".git/refs/heads/main").exists());
    assert_eq!(git(repo.path(), &["show", &format!("{}:first.txt", saved.reference)]), "first");
    assert_eq!(git(repo.path(), &["rev-list", "--count", &saved.reference]), "1");
    assert!(repo.path().join("first.txt").exists());
}

#[test]
fn tagged_head_and_prior_recovery_refs_survive_repeated_checkpoints() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(true);
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    git(repo.path(), &["tag", "v1"]);
    std::fs::write(repo.path().join("source.txt"), "first").unwrap();
    let first = preserve_shared_checkout(repo.path(), "t-repeat").unwrap().unwrap();
    std::fs::write(repo.path().join("source.txt"), "second").unwrap();
    let second = preserve_shared_checkout(repo.path(), "t-repeat").unwrap().unwrap();
    assert_ne!(first.reference, second.reference);
    assert_eq!(git(repo.path(), &["rev-parse", "v1"]), head);
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(git(repo.path(), &["show", &format!("{}:source.txt", first.reference)]), "first");
    assert_eq!(git(repo.path(), &["show", &format!("{}:source.txt", second.reference)]), "second");
}

#[test]
fn checkpoint_records_deletions_and_renames_from_a_nested_directory() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(true);
    std::fs::create_dir(repo.path().join("nested")).unwrap();
    std::fs::rename(repo.path().join("source.txt"), repo.path().join("nested/renamed.txt")).unwrap();
    let saved = preserve_shared_checkout(&repo.path().join("nested"), "t-rename").unwrap().unwrap();
    assert_eq!(git(repo.path(), &["ls-tree", "-r", "--name-only", &saved.reference]), "nested/renamed.txt");
    assert_eq!(git(repo.path(), &["show", "HEAD:source.txt"]), "base");
}

#[test]
fn clean_or_bookkeeping_only_checkout_does_not_create_recovery_ref() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(true);
    assert!(preserve_shared_checkout(repo.path(), "t-clean").unwrap().is_none());
    std::fs::write(repo.path().join(".aid-lock"), "lock").unwrap();
    std::fs::write(repo.path().join("result-t-clean.md"), "persisted elsewhere").unwrap();
    std::fs::create_dir(repo.path().join("target")).unwrap();
    std::fs::write(repo.path().join("target/binary"), "build").unwrap();
    assert!(preserve_shared_checkout(repo.path(), "t-clean").unwrap().is_none());
    assert_eq!(git(repo.path(), &["for-each-ref", "--format=%(refname)", "refs/aid/recovery/"]), "");
}

#[test]
fn checkpoint_failure_never_changes_principal_index_or_head() {
    let _permit = crate::test_subprocess::acquire();
    let repo = repo(true);
    let head = git(repo.path(), &["rev-parse", "HEAD"]);
    let index = std::fs::read(repo.path().join(".git/index")).unwrap();
    // A ref at this prefix prevents Git from creating a child ref. Object
    // creation can succeed, but publication must fail rather than overwrite it.
    git(repo.path(), &["update-ref", "refs/aid/recovery/t-fail", &head]);
    std::fs::write(repo.path().join("source.txt"), "recover me").unwrap();
    assert!(preserve_shared_checkout(repo.path(), "t-fail").is_err());
    assert_eq!(std::fs::read(repo.path().join(".git/index")).unwrap(), index);
    assert_eq!(git(repo.path(), &["rev-parse", "HEAD"]), head);
    assert_eq!(std::fs::read_to_string(repo.path().join("source.txt")).unwrap(), "recover me");
}

#[test]
fn non_git_directory_is_left_untouched() {
    let _permit = crate::test_subprocess::acquire();
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(dir.path().join("result.txt"), "result").unwrap();
    assert!(preserve_shared_checkout(dir.path(), "t-no-git").unwrap().is_none());
    assert_eq!(std::fs::read_dir(dir.path()).unwrap().count(), 1);
}
