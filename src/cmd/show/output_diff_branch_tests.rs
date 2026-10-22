// Tests for branch-wide diff rendering in aid show.
// Uses shared diff fixtures from the parent test module.

use super::*;

#[test]
fn branch_diff_signal_shows_branch_totals_when_start_sha_is_ahead_of_base() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());

    // Create an early commit on the task branch
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    write_and_commit(repo.path(), "early.txt", "early\n", "early");

    // start_sha is after the early commit
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    // Add another commit that the task just made
    write_and_commit(repo.path(), "task.txt", "task\n", "task");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture(
        "t-branch-diff",
        repo.path(),
        Some(&start_sha),
        TaskStatus::Done,
    );
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();

    assert!(
        text.contains("This branch carries earlier commits that are not in the diff above"),
        "got: {text}"
    );
    assert!(text.contains("Whole branch:"), "got: {text}");
    assert!(
        text.contains("2 files changed"),
        "stat should have total changed files: {text}"
    );
    assert!(
        text.contains("--diff --branch"),
        "notice must name the command that shows it: {text}"
    );
}

/// A clone that never created a local `main` still has `origin/main`. Resolving the base
/// to the bare name would make `git merge-base main HEAD` exit 128, and the branch view
/// would render empty while claiming to be the whole branch.
#[test]
fn branch_base_resolves_through_origin_when_no_local_default_branch_exists() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    let main_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);

    // Stand up origin/main + origin/HEAD, then drop the local branch entirely.
    git(
        repo.path(),
        &["update-ref", "refs/remotes/origin/main", &main_sha],
    );
    git(
        repo.path(),
        &[
            "symbolic-ref",
            "refs/remotes/origin/HEAD",
            "refs/remotes/origin/main",
        ],
    );
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    git(repo.path(), &["branch", "-D", "main"]);
    assert_eq!(
        branch_base_ref(repo.path().to_str().unwrap()).as_deref(),
        Some("origin/main"),
        "base must keep the remote prefix that actually resolves"
    );

    write_and_commit(repo.path(), "delivered.txt", "the actual work\n", "deliver");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture(
        "t-origin-base",
        repo.path(),
        Some(&start_sha),
        TaskStatus::Done,
    );
    store.insert_task(&task).unwrap();

    let text = diff_text(&store, task.id.as_str()).unwrap();
    assert!(
        text.contains("carries earlier commits"),
        "notice needs a resolvable base: {text}"
    );

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(
        !whole_branch.contains("Cannot locate this branch's base"),
        "base was resolvable: {whole_branch}"
    );
    assert!(
        whole_branch.contains("delivered.txt"),
        "branch view must reach the earlier commit: {whole_branch}"
    );
}

/// No default branch of any name resolves. The branch view must announce that it cannot
/// be built rather than labelling a fallback diff "whole branch".
#[test]
fn branch_mode_says_so_when_no_base_ref_resolves() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());
    git(repo.path(), &["checkout", "-b", "task-branch"]);
    git(repo.path(), &["branch", "-D", "main"]);
    assert_eq!(
        branch_base_ref(repo.path().to_str().unwrap()),
        None,
        "no base should resolve here"
    );

    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture("t-no-base", repo.path(), Some(&start_sha), TaskStatus::Done);
    store.insert_task(&task).unwrap();

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(
        whole_branch.contains("Cannot locate this branch's base"),
        "must not fake a branch view: {whole_branch}"
    );
    assert!(
        !whole_branch.contains("whole branch"),
        "no scope label it cannot honour: {whole_branch}"
    );
    // The message promises this task's own changes; the diff has to actually contain them.
    assert!(
        whole_branch.contains("sliver.txt"),
        "the promised task-scoped fallback must be real: {whole_branch}"
    );
    assert!(
        whole_branch.contains("trailing tweak"),
        "task-scoped fallback must carry content: {whole_branch}"
    );
}

/// The sliver that misled a reviewer on t-325bd9df: the task's own diff is honest but
/// tiny, and the delivered file sits in an earlier commit on the same branch.
#[test]
fn branch_mode_shows_earlier_commits_that_task_diff_omits() {
    let repo = tempfile::tempdir().unwrap();
    init_repo(repo.path());

    git(repo.path(), &["checkout", "-b", "task-branch"]);
    write_and_commit(repo.path(), "delivered.txt", "the actual work\n", "deliver");
    let start_sha = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    write_and_commit(repo.path(), "sliver.txt", "trailing tweak\n", "sliver");

    let store = Arc::new(Store::open_memory().unwrap());
    let task = task_fixture(
        "t-branch-mode",
        repo.path(),
        Some(&start_sha),
        TaskStatus::Done,
    );
    store.insert_task(&task).unwrap();

    let task_only = diff_text(&store, task.id.as_str()).unwrap();
    assert!(
        task_only.contains("sliver.txt"),
        "task diff must show its own commit: {task_only}"
    );
    assert!(
        !task_only.contains("the actual work"),
        "task diff is scoped to start_sha..HEAD by design: {task_only}"
    );

    let whole_branch = diff_text_branch(&store, task.id.as_str()).unwrap();
    assert!(
        whole_branch.contains("delivered.txt"),
        "branch diff must reach the earlier commit: {whole_branch}"
    );
    assert!(
        whole_branch.contains("the actual work"),
        "branch diff must include its content: {whole_branch}"
    );
    assert!(
        whole_branch.contains("sliver.txt"),
        "branch diff must still include this task's own commit: {whole_branch}"
    );
}
