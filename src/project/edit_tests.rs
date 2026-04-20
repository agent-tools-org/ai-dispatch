// Tests for targeted `.aid/project.toml` edits used by batch GitButler prompts.
// Covers minimal file creation plus in-place updates of project-scoped settings.
// Deps: super helpers, std::fs/path, tempfile.

use super::{
    project_path_in_repo, upsert_gitbutler_mode, upsert_gitbutler_prompt_suppressed,
};
use std::fs;

#[test]
fn upsert_gitbutler_mode_creates_minimal_project_file() {
    let repo = tempfile::tempdir().unwrap();

    let path = upsert_gitbutler_mode(repo.path(), "auto").unwrap();

    assert_eq!(path, project_path_in_repo(repo.path()));
    let contents = fs::read_to_string(path).unwrap();
    assert!(contents.contains("[project]"));
    assert!(contents.contains("id = "));
    assert!(contents.contains("gitbutler = \"auto\""));
}

#[test]
fn upsert_gitbutler_mode_replaces_existing_value_once() {
    let repo = tempfile::tempdir().unwrap();
    let path = project_path_in_repo(repo.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "[project]\nid = \"demo\"\ngitbutler = \"off\"\nverify = \"cargo test\"\n",
    )
    .unwrap();

    upsert_gitbutler_mode(repo.path(), "auto").unwrap();

    let contents = fs::read_to_string(path).unwrap();
    assert_eq!(contents.matches("gitbutler = ").count(), 1);
    assert!(contents.contains("gitbutler = \"auto\""));
    assert!(contents.contains("verify = \"cargo test\""));
}

#[test]
fn upsert_gitbutler_prompt_suppressed_inserts_inside_project_section() {
    let repo = tempfile::tempdir().unwrap();
    let path = project_path_in_repo(repo.path());
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    fs::write(
        &path,
        "[project]\nid = \"demo\"\n\n[audit]\nauto = true\n",
    )
    .unwrap();

    upsert_gitbutler_prompt_suppressed(repo.path(), true).unwrap();

    let contents = fs::read_to_string(path).unwrap();
    let project_idx = contents.find("[project]").unwrap();
    let suppress_idx = contents.find("suppress_gitbutler_prompt = true").unwrap();
    let audit_idx = contents.find("[audit]").unwrap();
    assert!(project_idx < suppress_idx);
    assert!(suppress_idx < audit_idx);
}
