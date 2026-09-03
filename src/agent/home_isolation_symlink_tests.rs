// Regression tests for leaked installer symlinks and safe negative controls.
// Exports: isolated-home symlink repair test coverage.
// Deps: home isolation API, tempfile, and std::fs.

use super::super::home_isolation::{reconcile_leaked_symlinks, IsolatedHomeGuard};
use std::fs;
use std::path::Path;

#[cfg(unix)]
#[test]
fn drop_rewrites_passthrough_installer_symlink_and_preserves_negative_controls() {
    let fixture = tempfile::tempdir().expect("fixture");
    let aid_home = tempfile::tempdir().expect("aid home");
    let _aid_guard = crate::paths::AidHomeGuard::set(aid_home.path());
    let real_home = fixture.path().join("real-home");
    let tool = real_home.join(".local/share/tool/v1/tool");
    fs::create_dir_all(tool.parent().expect("tool parent")).expect("tool dirs");
    fs::write(&tool, "tool payload").expect("tool payload");

    let (iso_path, unrelated_link, missing_link) = {
        let guard = IsolatedHomeGuard::create_from_home(Some(&real_home), None).expect("guard");
        let iso_path = guard.path().to_path_buf();
        let bin = iso_path.join(".local/bin");
        fs::create_dir_all(&bin).expect("isolated bin");
        let installed = bin.join("tool");
        std::os::unix::fs::symlink(iso_path.join(".local/share/tool/v1/tool"), &installed)
            .expect("installer symlink");

        let real_bin = real_home.join(".local/bin");
        let unrelated_link = real_bin.join("unrelated");
        std::os::unix::fs::symlink("/nonexistent/elsewhere", &unrelated_link)
            .expect("unrelated link");
        let unrelated_target = fs::read_link(&unrelated_link).expect("unrelated target");

        let missing_link = real_bin.join("missing");
        std::os::unix::fs::symlink(
            iso_path.join(".local/share/tool/v9/missing"),
            &missing_link,
        )
        .expect("missing link");

        drop(guard);
        assert_eq!(fs::read_link(&unrelated_link).expect("unrelated survives"), unrelated_target);
        (iso_path, unrelated_link, missing_link)
    };

    let repaired = real_home.join(".local/bin/tool");
    assert_eq!(fs::read_link(&repaired).expect("repaired target"), tool);
    assert_eq!(fs::read_to_string(repaired).expect("repaired resolves"), "tool payload");
    assert!(unrelated_link.exists() || fs::symlink_metadata(&unrelated_link).is_ok());
    assert_eq!(
        fs::read_link(&missing_link).expect("missing link survives"),
        iso_path.join(".local/share/tool/v9/missing")
    );
}

#[cfg(unix)]
#[test]
fn reconcile_uses_path_boundaries_and_leaves_missing_targets_untouched() {
    let fixture = tempfile::tempdir().expect("fixture");
    let real_home = fixture.path().join("real-home");
    let iso_home = fixture.path().join("iso-home");
    let bin = real_home.join(".local/bin");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&iso_home).expect("iso");
    let outside = bin.join("outside");
    std::os::unix::fs::symlink(Path::new("/nonexistent/elsewhere"), &outside)
        .expect("outside link");
    let outside_target = fs::read_link(&outside).expect("outside target");
    let missing = bin.join("missing");
    std::os::unix::fs::symlink(iso_home.join("missing"), &missing).expect("missing link");
    let missing_target = fs::read_link(&missing).expect("missing target");

    reconcile_leaked_symlinks(&iso_home, &real_home).expect("reconcile");

    assert_eq!(fs::read_link(outside).expect("outside survives"), outside_target);
    assert_eq!(fs::read_link(missing).expect("missing survives"), missing_target);
}
