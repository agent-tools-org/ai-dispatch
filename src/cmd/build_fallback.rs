// Detect cargo target-dir permission failures and choose a sandbox-safe fallback.
// Exports: target permission detection, fallback path, and digest note helpers.
// Deps: std path/env only.

use std::path::PathBuf;

/// True when cargo failed because the chosen target dir (or a cargo lock path
/// prefixed by it) was not writable. Keyed off cargo's real OS error, not a
/// preflight write probe — sandbox blocks are cargo-path-specific.
pub(crate) fn target_dir_permission_blocked(stderr_lines: &[String], target_dir: &str) -> bool {
    stderr_lines
        .iter()
        .any(|line| is_permission_block_for_target(line, target_dir))
}

/// System temp `aid-build-target/<cwd-key>` — writable inside agent OS sandboxes
/// that allow temp dirs. Prefer temp over `./target` because some sandboxes
/// block cargo writes under the worktree while still allowing plain file creation.
pub(crate) fn sandbox_fallback_target_dir() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let key = cwd_key(&cwd);
    let dir = std::env::temp_dir().join("aid-build-target").join(key);
    Some(dir.to_string_lossy().into_owned())
}

fn cwd_key(cwd: &std::path::Path) -> String {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let mut hasher = DefaultHasher::new();
    cwd.hash(&mut hasher);
    let name = cwd
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("project");
    format!("{name}-{:x}", hasher.finish())
}

pub(crate) fn fallback_digest_note(from: &str, to: &str) -> String {
    format!("note: CARGO_TARGET_DIR unwritable; fell back from {from} to {to}")
}

pub(crate) fn should_retry_with_fallback(
    success: bool,
    stderr_lines: &[String],
    target_dir: Option<&str>,
) -> Option<String> {
    if success {
        return None;
    }
    let chosen = target_dir?;
    if !target_dir_permission_blocked(stderr_lines, chosen) {
        return None;
    }
    let fallback = sandbox_fallback_target_dir()?;
    if PathBuf::from(&fallback) == PathBuf::from(chosen) {
        return None;
    }
    Some(fallback)
}

fn is_permission_block_for_target(line: &str, target_dir: &str) -> bool {
    let trimmed = line.trim();
    if !is_permission_os_error(trimmed) {
        return false;
    }
    let Some(path) = extract_at_path(trimmed) else {
        return false;
    };
    path_matches_target(path, target_dir)
}

fn is_permission_os_error(line: &str) -> bool {
    // Sandbox: EPERM (os error 1). Read-only dir simulation: EACCES (os error 13).
    line.contains("Operation not permitted (os error 1)")
        || line.contains("Permission denied (os error 13)")
}

fn extract_at_path(line: &str) -> Option<&str> {
    let (_, rest) = line.split_once("at path \"")?;
    let (path, _) = rest.split_once('"')?;
    Some(path)
}

fn path_matches_target(error_path: &str, target_dir: &str) -> bool {
    let target = target_dir.trim_end_matches('/');
    error_path == target || error_path.starts_with(target)
}

#[cfg(test)]
#[path = "build_fallback_tests.rs"]
mod tests;
