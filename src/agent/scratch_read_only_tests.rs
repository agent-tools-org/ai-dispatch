// Read-only root overlap regressions using canonical filesystem and Git layouts.
// Exercises launch preparation with nested task homes, targets and symlinks.
// Deps: parent scratch helpers, AidHomeGuard, tempfile and git CLI.

use super::*;

fn repository(path: &Path) {
    fs::create_dir_all(path).unwrap();
    assert!(
        Command::new("git")
            .args(["init", "-b", "main"])
            .arg(path)
            .output()
            .unwrap()
            .status
            .success()
    );
}

fn rejected(repo: &Path, target: Option<&Path>, scratch: &Path) {
    let result = prepare_launch_roots(
        AgentKind::Codex,
        repo.to_str(),
        target.and_then(Path::to_str),
        scratch,
        &TaskId("t-overlap".into()),
        true,
    );
    let error = result.expect_err("overlapping roots must fail read-only launch");
    assert!(format!("{error:#}").contains("overlap"), "{error:#}");
}

#[test]
fn fix_round_one_read_only_rejects_nested_task_directory() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    repository(temp.path());
    let scratch = tempfile::tempdir().unwrap();
    rejected(temp.path(), None, scratch.path());
}

#[test]
fn fix_round_one_read_only_rejects_task_directory_containing_checkout() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let repo = crate::paths::task_dir("t-overlap").join("checkout");
    repository(&repo);
    let scratch = tempfile::tempdir().unwrap();
    rejected(&repo, None, scratch.path());
}

#[test]
fn fix_round_one_read_only_rejects_ancestor_target() {
    let temp = tempfile::tempdir().unwrap();
    let home = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(home.path());
    let repo = temp.path().join("repo");
    repository(&repo);
    let scratch = create_temp_dir(home.path()).unwrap();
    rejected(&repo, Some(temp.path()), &scratch);
}

#[test]
fn fix_round_one_read_only_rejects_symlink_and_parent_traversal_roots() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let repo = temp.path().join("repo");
    repository(&repo);
    let target = repo.join("target");
    fs::create_dir(&target).unwrap();
    let link = temp.path().join("target-link");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    rejected(&repo, Some(&link), &scratch);
    rejected(&repo, Some(&target.join("..")), &scratch);
}

#[test]
fn fix_round_one_read_only_rejects_common_git_directory_from_subdirectory() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let repo = temp.path().join("repo");
    let wt = temp.path().join("worktree");
    repository(&repo);
    assert!(
        Command::new("git")
            .current_dir(&repo)
            .args([
                "-c",
                "user.name=Test",
                "-c",
                "user.email=test@example.com",
                "commit",
                "--allow-empty",
                "-m",
                "base"
            ])
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(
        Command::new("git")
            .current_dir(&repo)
            .args(["worktree", "add", "-b", "audit"])
            .arg(&wt)
            .output()
            .unwrap()
            .status
            .success()
    );
    let sub = wt.join("sub");
    fs::create_dir(&sub).unwrap();
    let scratch = create_temp_dir(temp.path()).unwrap();
    rejected(&sub, Some(&repo.join(".git")), &scratch);
    rejected(&sub, Some(&wt.join("sub")), &scratch);
    rejected(&sub, Some(&repo), &scratch);
}
