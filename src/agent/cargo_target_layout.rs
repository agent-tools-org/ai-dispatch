// Project-aware selection for an ambient Cargo target root.
// Exports CargoTargetLayout and layout_for_project() to the agent env module.
// Deps: project identity resolution and filesystem-safe project identifiers.

use std::path::{Path, PathBuf};

const BASE_TARGET_DIR_NAME: &str = "_base";

pub(super) struct CargoTargetLayout {
    pub(super) source: PathBuf,
    pub(super) branch_root: PathBuf,
}

pub(super) fn layout_for_project(
    branch_root: PathBuf,
    project_dir: Option<&str>,
) -> CargoTargetLayout {
    let branch_root = project_dir
        .and_then(|dir| routed_root(&branch_root, Path::new(dir)))
        .unwrap_or(branch_root);
    let source = branch_root.join(BASE_TARGET_DIR_NAME);
    CargoTargetLayout { source, branch_root }
}

fn routed_root(configured_root: &Path, project_dir: &Path) -> Option<PathBuf> {
    let caller_id = crate::project::current_project_id()?;
    let target_id = crate::project::resolve_project_id(project_dir)?;
    if caller_id == target_id {
        return None;
    }
    if crate::sanitize::validate_name(&target_id, "project").is_err() {
        return None;
    }
    rewrite_project_namespace(configured_root, &caller_id, &target_id)
}

fn rewrite_project_namespace(
    configured_root: &Path,
    caller_id: &str,
    target_id: &str,
) -> Option<PathBuf> {
    let caller_root = configured_root.ancestors().find(|ancestor| {
        ancestor.file_name().and_then(|name| name.to_str()) == Some(caller_id)
            && ancestor.parent().is_some_and(is_cargo_target_namespace)
    })?;
    Some(caller_root.parent()?.join(target_id))
}

fn is_cargo_target_namespace(path: &Path) -> bool {
    matches!(
        path.file_name().and_then(|name| name.to_str()),
        Some(".cargo-target" | "cargo-target")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unrelated_custom_roots_are_not_rewritten() {
        let configured = PathBuf::from("/tmp/custom-cache");
        let layout = layout_for_project(configured.clone(), Some("/not/a/repo"));

        assert_eq!(layout.branch_root, configured);
    }

    #[test]
    fn nested_caller_branch_is_dropped_when_project_changes() {
        let configured = Path::new("/cache/.cargo-target/caller/feature-one");

        let routed = rewrite_project_namespace(configured, "caller", "target").unwrap();

        assert_eq!(routed, PathBuf::from("/cache/.cargo-target/target"));
    }

    #[test]
    fn arbitrary_custom_cache_is_not_treated_as_a_project_namespace() {
        let configured = Path::new("/cache/custom/caller/feature-one");

        assert_eq!(rewrite_project_namespace(configured, "caller", "target"), None);
    }
}
