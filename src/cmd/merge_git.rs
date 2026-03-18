// Git helpers for `aid merge`.
// Exports: resolve_repo_dir, commits_ahead, auto_commit_uncommitted, git_merge_branch, MergeResult, remove_worktree, run_verify_in_worktree.
// Deps: std::fs, std::path::Path, std::process::Command.

use std::fs;
use std::path::Path;
use std::process::Command;

pub(crate) fn resolve_repo_dir(repo_path: Option<&str>, worktree_path: Option<&str>) -> String {
    if let Some(repo) = repo_path {
        return repo.to_string();
    }
    if let Some(wt) = worktree_path
        && let Ok(out) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--show-toplevel"])
            .output()
        && out.status.success()
    {
        let toplevel = String::from_utf8_lossy(&out.stdout).trim().to_string();
        if let Ok(common) = Command::new("git")
            .args(["-C", wt, "rev-parse", "--git-common-dir"])
            .output()
            && common.status.success()
        {
            let common_dir =
                String::from_utf8_lossy(&common.stdout).trim().to_string();
            let common_path = Path::new(&common_dir);
            if let Some(parent) = common_path.parent()
                && parent.join(".git").exists()
            {
                return parent.to_string_lossy().to_string();
            }
        }
        return toplevel;
    }
    ".".to_string()
}

pub(crate) fn commits_ahead(repo_dir: &str, branch: &str) -> u32 {
    let out = Command::new("git")
        .args(["-C", repo_dir, "rev-list", "--count", &format!("HEAD..{branch}")])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            String::from_utf8_lossy(&o.stdout).trim().parse().unwrap_or(0)
        }
        _ => 0,
    }
}

pub(crate) fn auto_commit_uncommitted(wt_path: &str, branch: &str) -> bool {
    let status = Command::new("git")
        .args(["-C", wt_path, "status", "--porcelain"])
        .output();
    let has_changes = match status {
        Ok(o) if o.status.success() => !o.stdout.is_empty(),
        _ => return false,
    };
    if !has_changes {
        return false;
    }
    eprintln!("[aid] Worktree has uncommitted changes — auto-committing on {branch}");
    let _ = Command::new("git")
        .args(["-C", wt_path, "add", "-A"])
        .output();
    let out = Command::new("git")
        .args([
            "-C",
            wt_path,
            "commit",
            "-m",
            "chore: auto-commit agent changes before merge",
        ])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Auto-committed uncommitted changes");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!(
                "[aid] Warning: auto-commit failed: {}",
                stderr.lines().next().unwrap_or("")
            );
            false
        }
        Err(e) => {
            eprintln!("[aid] Warning: auto-commit failed: {e}");
            false
        }
    }
}

pub(crate) fn git_merge_branch(repo_dir: &str, branch: &str) -> MergeResult {
    let head_before = Command::new("git")
        .args(["-C", repo_dir, "rev-parse", "HEAD"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());

    let stashed = stash_local_changes(repo_dir);

    let output = Command::new("git")
        .args(["-C", repo_dir, "merge", branch, "--no-edit"])
        .output();
    let merge_result = match output {
        Ok(o) if o.status.success() => {
            let stdout = String::from_utf8_lossy(&o.stdout);
            if stdout.contains("Already up to date") {
                MergeResult::AlreadyUpToDate
            } else {
                let head_after = Command::new("git")
                    .args(["-C", repo_dir, "rev-parse", "HEAD"])
                    .output()
                    .ok()
                    .filter(|o| o.status.success())
                    .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string());
                if head_before == head_after {
                    MergeResult::AlreadyUpToDate
                } else {
                    MergeResult::Merged
                }
            }
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr).to_string();
            MergeResult::Failed(stderr)
        }
        Err(e) => MergeResult::Failed(e.to_string()),
    };

    if stashed {
        if pop_stash(repo_dir) {
            eprintln!("[aid] Restored local changes");
        } else {
            eprintln!("[aid] Your stashed local changes conflict with the merge. Resolve with: git stash pop");
        }
    }
    merge_result
}

pub(crate) enum MergeResult {
    Merged,
    AlreadyUpToDate,
    Failed(String),
}

fn stash_local_changes(repo_dir: &str) -> bool {
    let dirty = Command::new("git")
        .args(["-C", repo_dir, "status", "--porcelain"])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| !o.stdout.is_empty())
        .unwrap_or(false);
    if !dirty {
        return false;
    }
    eprintln!("[aid] Stashing local changes before merge...");
    match Command::new("git")
        .args(["-C", repo_dir, "stash", "push", "-m", "aid: auto-stash before merge"])
        .output()
    {
        Ok(o) if o.status.success() => true,
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            eprintln!("[aid] Warning: failed to stash local changes: {}", stderr.lines().next().unwrap_or(""));
            false
        }
        Err(e) => {
            eprintln!("[aid] Warning: failed to stash local changes: {e}");
            false
        }
    }
}

fn pop_stash(repo_dir: &str) -> bool {
    match Command::new("git")
        .args(["-C", repo_dir, "stash", "pop"])
        .output()
    {
        Ok(o) => o.status.success(),
        Err(_) => false,
    }
}

/// Check if a path is safe to delete: must resolve to /tmp/aid-wt-* (or /private/tmp/aid-wt-* on macOS).
/// This is the sandbox guard — NEVER delete a directory that fails this check.
pub fn is_safe_worktree_path(wt_path: &str) -> bool {
    if !Path::new(wt_path).is_absolute() {
        return false;
    }
    // Canonicalize to resolve symlinks (macOS: /tmp → /private/tmp)
    let canonical = match Path::new(wt_path).canonicalize() {
        Ok(p) => p,
        // If path doesn't exist, check the raw string as fallback
        Err(_) => return wt_path.starts_with("/tmp/aid-wt-")
            || wt_path.starts_with("/private/tmp/aid-wt-"),
    };
    let s = canonical.to_string_lossy();
    s.starts_with("/tmp/aid-wt-") || s.starts_with("/private/tmp/aid-wt-")
}

pub fn remove_worktree(repo_dir: &str, wt_path: &str) {
    // SANDBOX: refuse to touch anything outside /tmp/aid-wt-*
    if !is_safe_worktree_path(wt_path) {
        eprintln!(
            "[aid] SAFETY: refusing to remove '{}' — not an aid worktree path. \
             Only /tmp/aid-wt-* paths are allowed.",
            wt_path
        );
        return;
    }

    let result = Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    match result {
        Ok(o) if o.status.success() => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            return;
        }
        _ => {}
    }

    // Fallback: rm -rf, but ONLY after sandbox validation above
    match fs::remove_dir_all(wt_path) {
        Ok(()) => {
            eprintln!("[aid] Cleaned up worktree {wt_path}");
            let _ = Command::new("git")
                .args(["-C", repo_dir, "worktree", "prune"])
                .output();
        }
        Err(e) => eprintln!("[aid] Warning: failed to remove {wt_path}: {e}"),
    }
}

pub(crate) fn run_verify_in_worktree(wt: &str, verify: Option<&str>) {
    let verify_cmd = match verify {
        Some("auto") | None => "cargo check",
        Some(cmd) => cmd,
    };
    let output = build_verify_command(verify_cmd)
        .and_then(|mut cmd| cmd.current_dir(wt).output())
        .map_err(|e| e.to_string());
    match output {
        Ok(o) if !o.status.success() => {
            eprintln!("[aid] Warning: `{verify_cmd}` failed in worktree {wt}");
            let stderr = String::from_utf8_lossy(&o.stderr);
            for line in stderr.lines().take(5) {
                eprintln!("  {}", line);
            }
        }
        Err(e) => eprintln!("[aid] Warning: could not run `{verify_cmd}`: {e}"),
        _ => {}
    }
}

fn build_verify_command(command: &str) -> std::io::Result<Command> {
    let mut parts = command.split_whitespace();
    let Some(program) = parts.next() else {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "verify command is empty",
        ));
    };
    let args: Vec<&str> = parts.collect();
    let mut cmd = Command::new(program);
    cmd.args(&args);
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::build_verify_command;

    #[test]
    fn build_verify_command_splits_simple_argv() {
        let cmd = build_verify_command("cargo test").unwrap();
        let debug = format!("{cmd:?}");
        assert!(debug.contains("\"cargo\""));
        assert!(debug.contains("\"test\""));
    }

    #[test]
    fn build_verify_command_keeps_shell_tokens_literal() {
        let cmd = build_verify_command("echo ok && false").unwrap();
        let debug = format!("{cmd:?}");
        assert!(debug.contains("\"&&\""));
        assert!(debug.contains("\"false\""));
    }
}
