// Regression tests for leaked installer symlinks and safe negative controls.
// Exports: isolated-home symlink repair test coverage.
// Deps: home isolation API, tempfile, and std::fs.

use super::super::home_isolation::{
    apply_repairs, find_doctor_symlinks, reconcile_leaked_symlinks, IsolatedHomeGuard,
    SymlinkRepair,
};
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

#[cfg(unix)]
#[test]
fn reconcile_does_not_strip_a_similar_task_id() {
    let fixture = tempfile::tempdir().expect("fixture");
    let real_home = fixture.path().join("real-home");
    let iso_home = fixture.path().join(".aid/tasks/t-abc/home");
    let bin = real_home.join(".local/bin");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&iso_home).expect("iso");
    let link = bin.join("tool");
    let old_target = fixture
        .path()
        .join(".aid/tasks/t-abc-evil/home/.local/bin/tool");
    std::os::unix::fs::symlink(&old_target, &link).expect("link");

    reconcile_leaked_symlinks(&iso_home, &real_home).expect("reconcile");

    assert_eq!(fs::read_link(&link).expect("link survives"), old_target);
}

#[cfg(unix)]
#[test]
fn doctor_ignores_tmp_home_paths_without_home_component() {
    let fixture = tempfile::tempdir().expect("fixture");
    let real_home = fixture.path().join("real-home");
    let aid_dir = fixture.path().join(".aid");
    let bin = real_home.join(".local/bin");
    fs::create_dir_all(&bin).expect("bin");
    let link = bin.join("tool");
    let old_target = aid_dir.join("tmp_home/iso-123/not-home/tool");
    std::os::unix::fs::symlink(&old_target, &link).expect("link");

    let repairs = find_doctor_symlinks(&real_home, &aid_dir).expect("scan");

    assert!(repairs.is_empty());
    assert_eq!(fs::read_link(&link).expect("link survives"), old_target);
}

#[cfg(unix)]
#[test]
fn reconcile_rejects_parent_dir_in_rewritten_rest() {
    let fixture = tempfile::tempdir().expect("fixture");
    let real_home = fixture.path().join("real-home");
    let iso_home = fixture.path().join("iso-home");
    let bin = real_home.join(".local/bin");
    fs::create_dir_all(&bin).expect("bin");
    fs::create_dir_all(&iso_home).expect("iso");
    let link = bin.join("tool");
    let old_target = iso_home.join("../../outside/tool");
    std::os::unix::fs::symlink(&old_target, &link).expect("link");

    reconcile_leaked_symlinks(&iso_home, &real_home).expect("reconcile");

    assert_eq!(fs::read_link(&link).expect("link survives"), old_target);
}

#[cfg(unix)]
#[test]
fn doctor_rejects_parent_dir_in_rewritten_rest() {
    let fixture = tempfile::tempdir().expect("fixture");
    let real_home = fixture.path().join("real-home");
    let aid_dir = fixture.path().join(".aid");
    let bin = real_home.join(".local/bin");
    fs::create_dir_all(&bin).expect("bin");
    let link = bin.join("tool");
    let old_target = aid_dir.join("tasks/t-abc/home/../../outside/tool");
    std::os::unix::fs::symlink(&old_target, &link).expect("link");

    let repairs = find_doctor_symlinks(&real_home, &aid_dir).expect("scan");

    assert!(repairs.is_empty());
    assert_eq!(fs::read_link(&link).expect("link survives"), old_target);
}

#[cfg(unix)]
#[test]
fn apply_repairs_skips_a_link_replaced_before_rename() {
    let fixture = tempfile::tempdir().expect("fixture");
    let link = fixture.path().join("tool");
    let target = fixture.path().join("real-tool");
    fs::write(&target, "payload").expect("target");
    fs::write(&link, "operator file").expect("swapped file");
    let repair = SymlinkRepair {
        link_path: link.clone(),
        old_target: fixture.path().join("old-tool"),
        rewritten_target: target,
    };

    let repaired = apply_repairs(&[repair]).expect("apply");

    assert_eq!(repaired, 0);
    assert_eq!(fs::read_to_string(link).expect("file survives"), "operator file");
}
