// Git worktree layout helpers shared by agent adapters and sandbox wrapping.
// Exports linked-worktree gitdir and commondir resolution using std fs/path APIs.

use std::fs;
use std::path::{Path, PathBuf};

pub(crate) fn resolve_worktree_gitdir(dir: &Path) -> Option<PathBuf> {
    let git_path = dir.join(".git");
    let gitfile = worktree_gitfile(&git_path)?;
    let content = match fs::read_to_string(&gitfile) {
        Ok(content) => content,
        Err(err) => {
            eprintln!("warning: failed to read codex worktree gitfile: {err}");
            return None;
        }
    };
    let Some(raw_path) = content.lines().find_map(|line| line.strip_prefix("gitdir:")) else {
        eprintln!("warning: failed to parse codex worktree gitfile: {}", gitfile.display());
        return None;
    };
    let path = Path::new(raw_path.trim());
    let resolved = if path.is_absolute() { path.to_path_buf() } else { dir.join(path) };
    match fs::canonicalize(resolved) {
        Ok(path) => Some(path),
        Err(err) => {
            eprintln!("warning: failed to resolve codex worktree gitdir: {err}");
            None
        }
    }
}

pub(crate) fn read_commondir(gitdir: &Path) -> Option<PathBuf> {
    let content = fs::read_to_string(gitdir.join("commondir")).ok()?;
    let path = Path::new(content.trim());
    let resolved = if path.is_absolute() {
        path.to_path_buf()
    } else {
        gitdir.join(path)
    };
    fs::canonicalize(resolved).ok()
}

fn worktree_gitfile(git_path: &Path) -> Option<PathBuf> {
    let (gitfile, metadata) = resolve_git_path(git_path)?;
    if metadata.is_dir() {
        return None;
    }
    if metadata.is_file() {
        return Some(gitfile);
    }
    eprintln!(
        "warning: codex .git path is neither file nor directory: {}",
        gitfile.display()
    );
    None
}

fn resolve_git_path(git_path: &Path) -> Option<(PathBuf, fs::Metadata)> {
    let metadata = fs::symlink_metadata(git_path).ok()?;
    if metadata.file_type().is_symlink() {
        let resolved = fs::canonicalize(git_path).ok()?;
        let metadata = fs::metadata(&resolved).ok()?;
        return Some((resolved, metadata));
    }
    Some((git_path.to_path_buf(), metadata))
}
