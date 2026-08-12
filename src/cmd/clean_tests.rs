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
