// Backs up merge-local untracked files outside the repository.
// Exports collision-safe backup and restore operations.
// Deps: standard filesystem APIs and git file listing.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};

pub(crate) struct UntrackedBackup {
    pub(crate) root: PathBuf,
    pub(crate) paths: Vec<PathBuf>,
}

pub(crate) fn backup_untracked(repo_dir: &str) -> Result<Option<UntrackedBackup>, String> {
    let paths = untracked_paths(repo_dir)?;
    if paths.is_empty() {
        return Ok(None);
    }
    let root = unique_backup_root()?;
    for relative in &paths {
        let source = Path::new(repo_dir).join(relative);
        let destination = root.join(relative);
        if let Err(error) = copy_entry(&source, &destination) {
            let _ = fs::remove_dir_all(&root);
            return Err(format!("failed to back up untracked file {}: {error}", relative.display()));
        }
    }
    let mut removed = Vec::new();
    for relative in &paths {
        let source = Path::new(repo_dir).join(relative);
        if let Err(error) = fs::remove_file(&source) {
            for restored in &removed {
                let _ = copy_entry(&root.join(restored), &Path::new(repo_dir).join(restored));
            }
            let _ = fs::remove_dir_all(&root);
            return Err(format!("failed to clear untracked file {}: {error}", relative.display()));
        }
        removed.push(relative.clone());
    }
    Ok(Some(UntrackedBackup { root, paths }))
}

pub(crate) fn restore_backup_after_capture_failure<'a>(
    repo_dir: &str,
    backup: Option<&'a UntrackedBackup>,
) -> Option<&'a UntrackedBackup> {
    backup.filter(|backup| restore_untracked(repo_dir, backup).is_err())
}

pub(crate) fn restore_untracked(
    repo_dir: &str,
    backup: &UntrackedBackup,
) -> Result<(), String> {
    ensure_untracked_destinations_free(repo_dir, backup)?;
    for relative in &backup.paths {
        let destination = Path::new(repo_dir).join(relative);
        copy_entry(&backup.root.join(relative), &destination)?;
    }
    fs::remove_dir_all(&backup.root)
        .map_err(|error| format!("failed to remove local-change backup: {error}"))
}

pub(crate) fn ensure_untracked_destinations_free(
    repo_dir: &str,
    backup: &UntrackedBackup,
) -> Result<(), String> {
    let collisions: Vec<_> = backup
        .paths
        .iter()
        .filter(|relative| fs::symlink_metadata(Path::new(repo_dir).join(relative)).is_ok())
        .collect();
    if !collisions.is_empty() {
        let files = collisions
            .iter()
            .map(|path| path.display().to_string())
            .collect::<Vec<_>>()
            .join(", ");
        return Err(format!("untracked files left in backup {}: {files}", backup.root.display()));
    }
    Ok(())
}

fn untracked_paths(repo_dir: &str) -> Result<Vec<PathBuf>, String> {
    let output = Command::new("git")
        .args(["-C", repo_dir, "ls-files", "--others", "--exclude-standard", "-z"])
        .output()
        .map_err(|error| format!("failed to list untracked files: {error}"))?;
    if !output.status.success() {
        return Err(format!("failed to list untracked files: {}", first_error_line(&output.stderr)));
    }
    Ok(String::from_utf8_lossy(&output.stdout)
        .split('\0')
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
        .collect())
}

fn unique_backup_root() -> Result<PathBuf, String> {
    let timestamp = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|error| format!("failed to create local-change backup name: {error}"))?
        .as_nanos();
    let root = std::env::temp_dir().join(format!(
        "aid-merge-local-{}-{timestamp}",
        std::process::id()
    ));
    fs::create_dir(&root).map_err(|error| format!("failed to create local-change backup: {error}"))?;
    Ok(root)
}

fn copy_entry(source: &Path, destination: &Path) -> Result<(), String> {
    if let Some(parent) = destination.parent() {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file_type = fs::symlink_metadata(source)
        .map_err(|error| error.to_string())?
        .file_type();
    if file_type.is_symlink() {
        return copy_symlink(source, destination);
    }
    fs::copy(source, destination)
        .map(|_| ())
        .map_err(|error| error.to_string())
}

#[cfg(unix)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    std::os::unix::fs::symlink(fs::read_link(source).map_err(|error| error.to_string())?, destination)
        .map_err(|error| error.to_string())
}

#[cfg(windows)]
fn copy_symlink(source: &Path, destination: &Path) -> Result<(), String> {
    let target = fs::read_link(source).map_err(|error| error.to_string())?;
    if target.is_dir() {
        std::os::windows::fs::symlink_dir(target, destination)
    } else {
        std::os::windows::fs::symlink_file(target, destination)
    }
    .map_err(|error| error.to_string())
}

fn first_error_line(stderr: &[u8]) -> String {
    String::from_utf8_lossy(stderr)
        .lines()
        .next()
        .unwrap_or("unknown git error")
        .to_string()
}
