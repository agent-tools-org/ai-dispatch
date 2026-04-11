// Git repository root detection and nested-repo warnings.
// Exports explicit repo-root resolution for run/batch dispatch.
// Deps: git CLI, std path/process utilities.
use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Debug, PartialEq, Eq)]
struct NestedRepo {
    inner: PathBuf,
    outer: PathBuf,
}

pub(crate) fn resolve_git_root_string(path: &str) -> Result<String> {
    let root = resolve_git_root(Path::new(path))?;
    Ok(root.to_string_lossy().to_string())
}

pub(crate) fn resolve_explicit_repo_path(
    repo_root: Option<&str>,
    repo: Option<&str>,
) -> Result<Option<String>> {
    let Some(path) = repo_root.or(repo) else {
        return Ok(None);
    };
    resolve_git_root_string(path).map(Some)
}

pub(crate) fn warn_if_nested_repo(start_dir: &str) {
    let Ok(Some(nested)) = detect_nested_repo(Path::new(start_dir)) else {
        return;
    };
    aid_warn!("{}", nested_repo_warning(&nested));
}

fn detect_nested_repo(start_dir: &Path) -> Result<Option<NestedRepo>> {
    let inner = resolve_git_root(start_dir)?;
    let mut cursor = inner.parent().map(Path::to_path_buf);
    while let Some(dir) = cursor {
        if has_git_marker(&dir) {
            if is_submodule_path(&dir, &inner) {
                return Ok(None);
            }
            return Ok(Some(NestedRepo {
                inner,
                outer: dir,
            }));
        }
        cursor = dir.parent().map(Path::to_path_buf);
    }
    Ok(None)
}

fn resolve_git_root(path: &Path) -> Result<PathBuf> {
    let out = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .context("Failed to run git")?;
    anyhow::ensure!(out.status.success(), "Not a git repository: {}", path.display());
    let root = String::from_utf8_lossy(&out.stdout).trim().to_string();
    Ok(PathBuf::from(root))
}

fn has_git_marker(path: &Path) -> bool {
    let marker = path.join(".git");
    marker.is_dir() || marker.is_file()
}

fn is_submodule_path(outer: &Path, inner: &Path) -> bool {
    let gitmodules = outer.join(".gitmodules");
    if !gitmodules.is_file() {
        return false;
    }
    let Ok(out) = Command::new("git")
        .arg("-C")
        .arg(outer)
        .args(["config", "-f", ".gitmodules", "--get-regexp", r"^submodule\..*\.path$"])
        .output()
    else {
        return false;
    };
    if !out.status.success() {
        return false;
    }
    let inner = canonical_or_self(inner);
    String::from_utf8_lossy(&out.stdout)
        .lines()
        .filter_map(|line| line.split_once(' ').map(|(_, path)| outer.join(path.trim())))
        .any(|path| canonical_or_self(&path) == inner)
}

fn canonical_or_self(path: &Path) -> PathBuf {
    path.canonicalize().unwrap_or_else(|_| path.to_path_buf())
}

fn nested_repo_warning(nested: &NestedRepo) -> String {
    format!(
        "[aid] WARNING: nested git repos detected\n        inner: {} (remote: {})\n        outer: {} (remote: {})\n      Worktrees will use the INNER repo. Outer-repo changes are NOT visible.\n      To use the outer repo: re-run from {}, or pass --repo-root {}\n      To suppress: pass --repo-root {}",
        nested.inner.display(),
        remote_origin(&nested.inner).unwrap_or_else(|| "none".to_string()),
        nested.outer.display(),
        remote_origin(&nested.outer).unwrap_or_else(|| "none".to_string()),
        nested.outer.display(),
        nested.outer.display(),
        nested.inner.display(),
    )
}

fn remote_origin(repo: &Path) -> Option<String> {
    let out = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(["config", "--get", "remote.origin.url"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let value = String::from_utf8_lossy(&out.stdout).trim().to_string();
    (!value.is_empty()).then_some(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn detects_outer_repo_for_nested_non_submodule() {
        let temp = tempfile::tempdir().unwrap();
        let outer = temp.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        git_init(&outer);
        git_init(&inner);

        let nested = detect_nested_repo(&inner).unwrap().unwrap();

        assert_eq!(canonical_or_self(&nested.inner), canonical_or_self(&inner));
        assert_eq!(canonical_or_self(&nested.outer), canonical_or_self(&outer));
    }

    #[test]
    fn ignores_nested_repo_when_outer_declares_submodule_path() {
        let temp = tempfile::tempdir().unwrap();
        let outer = temp.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        git_init(&outer);
        git_init(&inner);
        fs::write(
            outer.join(".gitmodules"),
            "[submodule \"inner\"]\n\tpath = inner\n\turl = ../inner\n",
        )
        .unwrap();

        let nested = detect_nested_repo(&inner).unwrap();

        assert_eq!(nested, None);
    }

    #[test]
    fn explicit_repo_root_overrides_legacy_repo_path() {
        let temp = tempfile::tempdir().unwrap();
        let outer = temp.path().join("outer");
        let inner = outer.join("inner");
        fs::create_dir_all(&inner).unwrap();
        git_init(&outer);
        git_init(&inner);

        let resolved = resolve_explicit_repo_path(
            Some(outer.to_string_lossy().as_ref()),
            Some(inner.to_string_lossy().as_ref()),
        )
        .unwrap()
        .unwrap();

        assert_eq!(canonical_or_self(Path::new(&resolved)), canonical_or_self(&outer));
    }

    fn git_init(path: &Path) {
        let status = Command::new("git")
            .arg("-C")
            .arg(path)
            .args(["init", "-q"])
            .status()
            .unwrap();
        assert!(status.success());
    }
}
