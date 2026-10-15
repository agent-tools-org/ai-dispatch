// Resolve a checkout and its optional project config without reading caller cwd.
// Exports: resolve_project_in for dispatch; detect_project_in for config consumers.
// Deps: project loading, Git root discovery, linked-worktree discovery.
use super::{find_git_root_from, load_project, project_path_in_repo, worktree, ProjectConfig};
use std::path::{Path, PathBuf};

pub fn detect_project_in(start_dir: &Path) -> Option<ProjectConfig> {
    resolve_project_in(start_dir).1
}

pub(crate) fn resolve_project_in(start_dir: &Path) -> (Option<PathBuf>, Option<ProjectConfig>) {
    let Some(root) = start_dir.canonicalize().ok().and_then(|dir| find_git_root_from(&dir)) else {
        return (None, None);
    };
    let config = load_checkout_project(&root);
    (Some(root), config)
}

fn load_checkout_project(root: &Path) -> Option<ProjectConfig> {
    let mut path = project_path_in_repo(root);
    if !path.is_file() {
        if !root.join(".git").is_file() { return None; }
        path = project_path_in_repo(&worktree::main_working_tree_of(root)?);
    }
    load_project(&path).ok()
}
