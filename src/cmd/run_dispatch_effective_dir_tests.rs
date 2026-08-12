// Tests effective directory normalization during new task persistence.
// Exports: none.
// Deps: parent dispatch preparation module and tempfile.

use super::persistable_effective_dir;

#[test]
fn persistable_effective_dir_trims_recorded_whitespace() {
    let dir = tempfile::tempdir().unwrap();
    let padded = format!(" {} ", dir.path().display());

    assert_eq!(
        persistable_effective_dir(Some(&padded)).as_deref(),
        dir.path().to_str()
    );
}
