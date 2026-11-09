// Unit tests for Git rev-parse stderr classification in read-only settlement.
// Covers parent-directory and mount-boundary "not a repository" variants.
// Deps: read_only::rev_parse_stderr_is_not_repository.

use super::{classify_rev_parse_stderr, rev_parse_stderr_is_not_repository};

#[test]
fn read_only_git_classic_not_repository_stderr() {
    let stderr = "fatal: not a git repository (or any of the parent directories): /tmp/nope\n";
    assert!(rev_parse_stderr_is_not_repository(stderr));
    assert_eq!(classify_rev_parse_stderr(stderr).unwrap(), false);
}

#[test]
fn read_only_git_mount_boundary_not_repository_stderr() {
    let stderr = "fatal: not a git repository (or any parent up to mount point /)\n\
Stopping at filesystem boundary (GIT_DISCOVERY_ACROSS_FILESYSTEM not set).\n";
    assert!(rev_parse_stderr_is_not_repository(stderr));
    assert_eq!(classify_rev_parse_stderr(stderr).unwrap(), false);
}

#[test]
fn read_only_git_other_rev_parse_errors_are_not_missing_repository() {
    let stderr = "fatal: this operation must be run in a work tree\n";
    assert!(!rev_parse_stderr_is_not_repository(stderr));
    assert!(classify_rev_parse_stderr(stderr).is_err());
}

#[test]
fn read_only_git_missing_gitdir_is_a_broken_checkout() {
    let stderr = "fatal: not a git repository: /repo/.git/worktrees/gone\n";
    assert!(!rev_parse_stderr_is_not_repository(stderr));
    assert!(classify_rev_parse_stderr(stderr).is_err());
}
