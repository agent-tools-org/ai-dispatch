// Dependency hints gated by worktree state and missing install directories.
// Exports: missing_deps_hint(); deps: shared manifest scan and dependency mapping.
use super::{MISSING_DEPS_HINT, VERIFY_STATE_FILE, dep_names_for_manifest, manifest_parents};
use std::{collections::HashSet, fs, path::Path};

pub fn missing_deps_hint(worktree_dir: &Path) -> Option<&'static str> {
    let state = fs::read_to_string(worktree_dir.join(VERIFY_STATE_FILE)).ok()?;
    let mut fresh = false;
    let mut setup_defined = false;
    let mut linked_any = false;
    for line in state.lines() {
        if let Some(value) = line.strip_prefix("fresh=") {
            fresh = value == "1";
        } else if let Some(value) = line.strip_prefix("setup_defined=") {
            setup_defined = value == "1";
        } else if let Some(value) = line.strip_prefix("linked_any=") {
            linked_any = value == "1";
        }
    }
    if !fresh || setup_defined || linked_any {
        return None;
    }
    let deps = manifest_parents(worktree_dir, worktree_dir, 0, &mut HashSet::new()).ok()?;
    deps.into_iter()
        .any(|(parent, dep_name)| match dep_name {
            "target" => false,
            ".venv" | "venv" => dep_names_for_manifest("pyproject.toml")
                .iter()
                .all(|name| !parent.join(name).is_dir()),
            _ => !parent.join(dep_name).is_dir(),
        })
        .then_some(MISSING_DEPS_HINT)
}
