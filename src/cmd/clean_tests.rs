// Tests for cleanup reporting and bounded size measurement.
// Deps: clean, clean_size, tempfile, std::fs.

use rusqlite::params;

    use super::*;

    #[cfg(unix)]
    use std::os::unix::fs::MetadataExt;

    fn contains_path(paths: &[PathBuf], needle: &Path) -> bool {
        paths.iter().any(|path| path == needle)
    }

    #[test]
    fn legacy_tmp_worktree_path_is_collectable() {
        let worktree = tempfile::Builder::new()
            .prefix("aid-wt-clean-legacy-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut paths = Vec::new();

        collect_legacy_tmp_worktree_paths(Path::new("/tmp"), &mut paths).unwrap();

        assert!(contains_path(&paths, worktree.path()));
    }

    #[test]
    fn non_aid_tmp_path_is_rejected_by_clean_scan() {
        let worktree = tempfile::Builder::new()
            .prefix("not-aid-clean-")
            .tempdir_in("/tmp")
            .unwrap();
        let mut paths = Vec::new();

        collect_legacy_tmp_worktree_paths(Path::new("/tmp"), &mut paths).unwrap();

        assert!(!contains_path(&paths, worktree.path()));
        assert!(!is_aid_managed_worktree_path(worktree.path()));
    }

    #[test]
    fn clean_orphaned_shared_dirs_removes_unknown_workgroup_dirs() {
        let temp = tempfile::tempdir().unwrap();
        let _guard = crate::paths::AidHomeGuard::set(temp.path());
        let store = Store::open_memory().unwrap();
        crate::shared_dir::create_shared_dir("wg-orphanned").unwrap();
        crate::shared_dir::create_shared_dir("wg-known").unwrap();
        store.create_workgroup("Known", "", None, Some("wg-known")).unwrap();

        clean_orphaned_shared_dirs(&store, false).unwrap();

        assert!(crate::shared_dir::shared_dir_path("wg-orphanned").is_none());
        assert!(crate::shared_dir::shared_dir_path("wg-known").is_some());
    }

    #[cfg(unix)]
    #[test]
    fn failed_task_home_removal_does_not_abort_later_homes() {
        use std::os::unix::fs::PermissionsExt;

        let aid_home = tempfile::tempdir().unwrap();
        let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
        let store = Store::open_memory().unwrap();
        for id in ["t-failed-home", "t-good-home"] {
            store.db().execute(
                "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES (?1, 'codex', 'test', 'done', '2026-01-01T00:00:00Z')",
                params![id],
            ).unwrap();
        }

        let failed_home = crate::paths::task_dir("t-failed-home").join("home");
        fs::create_dir_all(&failed_home).unwrap();
        fs::write(failed_home.join("payload"), "keep").unwrap();
        fs::set_permissions(failed_home.parent().unwrap(), fs::Permissions::from_mode(0o500)).unwrap();

        let good_home = crate::paths::task_dir("t-good-home").join("home");
        fs::create_dir_all(&good_home).unwrap();
        fs::write(good_home.join("payload"), "remove").unwrap();

        let mut sizes = crate::cmd::clean_size::SizeTracker::new();
        let result = clean_isolated_task_homes(&store, false, &mut sizes);
        fs::set_permissions(failed_home.parent().unwrap(), fs::Permissions::from_mode(0o700)).unwrap();

        assert!(result.is_ok());
        assert!(failed_home.exists());
        assert!(!good_home.exists());
    }

    #[test]
    fn unresolved_real_home_does_not_abort_isolated_home_cleanup() {
        struct HomeGuard(Option<std::ffi::OsString>);

        impl Drop for HomeGuard {
            fn drop(&mut self) {
                match self.0.take() {
                    Some(home) => unsafe { std::env::set_var("HOME", home) },
                    None => unsafe { std::env::remove_var("HOME") },
                }
            }
        }

        let aid_home = tempfile::tempdir().unwrap();
        let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
        let store = Store::open_memory().unwrap();
        store.db().execute(
            "INSERT INTO tasks (id, agent, prompt, status, created_at) VALUES ('t-unresolved-home', 'codex', 'test', 'done', '2026-01-01T00:00:00Z')",
            [],
        ).unwrap();
        let home = crate::paths::task_dir("t-unresolved-home").join("home");
        fs::create_dir_all(&home).unwrap();
        let previous = std::env::var_os("HOME");
        let _home_guard = HomeGuard(previous);
        unsafe { std::env::set_var("HOME", aid_home.path().join("missing-home")) };

        let mut sizes = crate::cmd::clean_size::SizeTracker::new();
        let result = clean_isolated_task_homes(&store, false, &mut sizes);

        assert!(result.is_ok());
        assert!(home.exists());
    }

    #[test]
    fn bounded_dir_size_stops_at_entry_limit() {
        let temp = tempfile::tempdir().unwrap();
        fs::write(temp.path().join("first"), vec![b'a'; 7]).unwrap();
        fs::write(temp.path().join("second"), vec![b'b'; 11]).unwrap();
        fs::write(temp.path().join("third"), vec![b'c'; 13]).unwrap();

        let (bytes, entries) = crate::cmd::clean_size::get_dir_size_bounded(temp.path(), 10_000, 2).unwrap();

        assert_eq!(entries, 2);
        assert!(bytes >= 18);
    }

    #[cfg(unix)]
    #[test]
    fn size_tracker_counts_hardlinked_inode_once() {
        let temp = tempfile::tempdir().unwrap();
        let first = temp.path().join("first");
        let second = temp.path().join("second");
        fs::write(&first, vec![b'x'; 17]).unwrap();
        fs::hard_link(&first, &second).unwrap();

        let mut sizes = crate::cmd::clean_size::SizeTracker::new();
        assert_eq!(
            sizes.get_dir_size(temp.path()).unwrap(),
            first.metadata().unwrap().blocks().saturating_mul(512)
        );
    }

    #[test]
    fn truncated_session_hint_is_visible_and_actionable() {
        let hint = render_session_start_hint(
            crate::cmd::clean_cargo_target::ReclaimableSpaceEstimate {
                bytes: 128 * 1024 * 1024,
                truncated: true,
            },
        );

        assert!(hint.contains("at least 128.0 MB"));
        assert!(hint.contains("incomplete scan"));
        assert!(hint.contains("aid clean --worktrees --dry-run"));
    }
