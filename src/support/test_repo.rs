// Shared Git repository fixtures and unique test branch names.
// Exports: init_with_file, repo_with_file, and unique_name.
// Deps: test_env Git assertions, tempfile, std filesystem/time.

use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};
use super::test_env::git;

pub(crate) fn init_with_file(repo: &Path, file: &str, content: &str, message: &str) {
    git(repo, &["init", "-b", "main"]);
    git(repo, &["config", "user.email", "test@example.com"]);
    git(repo, &["config", "user.name", "Test User"]);
    std::fs::write(repo.join(file), content).expect("fixture file");
    git(repo, &["add", file]);
    git(repo, &["commit", "-m", message]);
}

pub(crate) fn repo_with_file(file: &str, content: &str, message: &str) -> tempfile::TempDir {
    let repo = tempfile::tempdir().expect("repo");
    init_with_file(repo.path(), file, content, message);
    repo
}

pub(crate) fn unique_name(prefix: &str) -> String {
    format!(
        "{prefix}-{}-{}",
        std::process::id(),
        SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()
    )
}

pub(crate) fn write_path(dir: &Path, path: &str, content: &str) {
    let file = dir.join(path);
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent).unwrap();
    }
    std::fs::write(file, content).unwrap();
}
