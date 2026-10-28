// Focused tests for factual post-run output presence detection.
// Exports no helpers; exercises transcript fallback used by hollow-output checks.
// Deps: run_lifecycle output/final-state helpers, paths, task domain types.

use super::run_lifecycle::{capture_final_worktree_state, output_has_content};
use crate::store::Store;
use crate::support::test_subprocess;
use crate::types::{Task, TaskId, TaskStatus};




use crate::support::test_env::git;

use crate::support::test_env::git_output as git_stdout;

fn task(id: &str) -> Task {
    crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Done)
}

#[test]
fn output_presence_accepts_plain_text_transcript() {
    let _temp = crate::support::test_env::TestHome::new();
    let task = task("t-transcript-output");
    let transcript = crate::support::paths::transcript_path(task.id.as_str());
    std::fs::create_dir_all(transcript.parent().unwrap()).unwrap();
    std::fs::write(&transcript, "plain transcript output\n".repeat(20)).unwrap();

    assert!(output_has_content(&task));
}

#[test]
fn output_presence_accepts_one_character() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.md");
    std::fs::write(&output_path, "好").unwrap();
    let mut task = task("t-short-multibyte");
    task.output_path = Some(output_path.to_string_lossy().to_string());

    assert!(output_has_content(&task));
}

#[test]
fn output_presence_rejects_whitespace_only_file() {
    let dir = tempfile::tempdir().unwrap();
    let output_path = dir.path().join("output.md");
    std::fs::write(&output_path, " \n\t").unwrap();
    let mut task = task("t-empty-output");
    task.output_path = Some(output_path.to_string_lossy().to_string());

    assert!(!output_has_content(&task));
}

#[test]
fn capture_final_worktree_state_records_switched_branch() {
    let _permit = test_subprocess::acquire();
    let repo = tempfile::tempdir().unwrap();
    git(repo.path(), &["init", "-b", "main"]);
    git(repo.path(), &["config", "user.email", "test@example.com"]);
    git(repo.path(), &["config", "user.name", "Test User"]);
    std::fs::write(repo.path().join("base.txt"), "base\n").unwrap();
    git(repo.path(), &["add", "base.txt"]);
    git(repo.path(), &["commit", "-m", "base"]);
    git(repo.path(), &["switch", "-c", "agent/final"]);
    std::fs::write(repo.path().join("final.txt"), "done\n").unwrap();
    git(repo.path(), &["add", "final.txt"]);
    git(repo.path(), &["commit", "-m", "final work"]);
    let head = git_stdout(repo.path(), &["rev-parse", "HEAD"]);
    let store = Store::open_memory().unwrap();
    let mut task = task("t-final-capture");
    task.worktree_path = Some(repo.path().to_string_lossy().to_string());
    task.worktree_branch = Some("dispatch/branch".to_string());
    store.insert_task(&task).unwrap();

    capture_final_worktree_state(&store, &task.id).unwrap();
    git(repo.path(), &["switch", "-c", "agent/later"]);
    capture_final_worktree_state(&store, &task.id).unwrap();

    let loaded = store.get_task(task.id.as_str()).unwrap().unwrap();
    assert_eq!(loaded.final_head_sha.as_deref(), Some(head.as_str()));
    assert_eq!(loaded.final_branch.as_deref(), Some("agent/final"));
}
