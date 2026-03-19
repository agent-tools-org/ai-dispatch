// Shared directory helpers for batch workgroups.
// Exports: create_shared_dir, cleanup_shared_dir, shared_dir_path.
// Deps: crate::paths, std::fs, tempfile tests.
use anyhow::Result;
use std::path::PathBuf;

pub fn create_shared_dir(workgroup_id: &str) -> Result<PathBuf> {
    let dir = shared_dir_base().join(workgroup_id);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

pub fn cleanup_shared_dir(workgroup_id: &str) {
    let dir = shared_dir_base().join(workgroup_id);
    let _ = std::fs::remove_dir_all(&dir);
}

pub fn shared_dir_path(workgroup_id: &str) -> Option<PathBuf> {
    let dir = shared_dir_base().join(workgroup_id);
    dir.exists().then_some(dir)
}

fn shared_dir_base() -> PathBuf {
    crate::paths::aid_dir().join("shared")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn create_shared_dir_creates_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        let path = create_shared_dir("wg-shared").expect("create shared dir");

        assert!(path.is_dir());
        assert_eq!(shared_dir_path("wg-shared"), Some(path));
    }

    #[test]
    fn cleanup_shared_dir_removes_directory() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        let path = create_shared_dir("wg-shared").expect("create shared dir");

        cleanup_shared_dir("wg-shared");

        assert!(!path.exists());
    }

    #[test]
    fn shared_dir_path_returns_none_when_missing() {
        let temp = tempfile::tempdir().expect("tempdir");
        let _guard = crate::paths::AidHomeGuard::set(temp.path());

        assert_eq!(shared_dir_path("wg-missing"), None);
    }
}
