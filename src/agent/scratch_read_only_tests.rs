// Read-only root overlap regressions using canonical filesystem and Git layouts.
// Exercises launch preparation with nested task homes, targets and symlinks.
// Deps: parent scratch helpers, AidHomeGuard, tempfile and git CLI.

use super::*;

#[test]
fn scope_cut_read_only_grants_only_empty_staging_outside_trusted_writers() {
    let home = crate::support::test_env::TestHome::new();
    let repo = home.path().join("repo");
    repository(&repo);
    let staging = create_temp_dir(home.path()).unwrap();
    let target = home.path().join("target");
    fs::create_dir(&target).unwrap();
    let task = TaskId("t-stage-only".into());
    let (roots, warnings) = prepare_launch_roots(
        AgentKind::Codex, repo.to_str(), target.to_str(), &staging, &task, true,
    ).unwrap();
    assert!(warnings.is_empty());
    assert_eq!(roots, vec![staging.clone()]);
    assert_eq!(fs::read_dir(staging).unwrap().count(), 0);
    for name in ["result.md", "output.md", "transcript.md"] {
        let trusted = crate::support::paths::task_dir(task.as_str()).join(name);
        assert!(!roots.iter().any(|root| trusted.starts_with(root)));
    }
}

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
    let roots: Vec<_> = target.into_iter().chain(std::iter::once(scratch)).map(Path::to_path_buf).collect();
    let result = validate_read_only_roots(repo.to_str(), &roots);
    let error = result.expect_err("overlapping roots must fail read-only launch");
    assert!(format!("{error:#}").contains("overlap"), "{error:#}");
}

#[test]
fn fix_round_one_read_only_rejects_nested_staging_directory() {
    let temp = crate::support::test_env::TestHome::new();
    repository(temp.path());
    let scratch = create_temp_dir(temp.path()).unwrap();
    rejected(temp.path(), None, &scratch);
}

#[test]
fn fix_round_one_read_only_rejects_staging_directory_containing_checkout() {
    let temp = crate::support::test_env::TestHome::new();
    let repo = crate::support::paths::task_dir("t-overlap").join("checkout");
    repository(&repo);
    rejected(&repo, None, temp.path());
}

#[test]
fn fix_round_one_read_only_rejects_ancestor_target() {
    let temp = tempfile::tempdir().unwrap();
    let home = crate::support::test_env::TestHome::new();
    let repo = temp.path().join("repo");
    repository(&repo);
    let scratch = create_temp_dir(home.path()).unwrap();
    rejected(&repo, Some(temp.path()), &scratch);
}

#[test]
fn fix_round_one_read_only_rejects_symlink_and_parent_traversal_roots() {
    let temp = crate::support::test_env::TestHome::new();
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
    let temp = crate::support::test_env::TestHome::new();
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
