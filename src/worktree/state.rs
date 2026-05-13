// Worktree state helpers shared by create/reuse flows.
// Exports: git/worktree lookup, lock-file helpers, branch/file inspection.
// Deps: anyhow, std::fs, std::path, std::process::Command.

use anyhow::{Context, Result};
use std::fs::OpenOptions;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

pub fn sync_cargo_lock(repo_dir: &Path, wt_path: &Path) {
    let src = repo_dir.join("Cargo.lock");
    let dst = wt_path.join("Cargo.lock");
    if src.exists() {
        let _ = std::fs::copy(&src, &dst);
    }
}

pub fn prune_worktrees(repo_dir: &Path) -> Result<()> {
    let prune_status = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "worktree", "prune"])
        .status()
        .context("Failed to run git worktree prune")?;
    anyhow::ensure!(prune_status.success(), "git worktree prune failed");
    Ok(())
}

pub fn existing_worktree_path(repo_dir: &Path, branch: &str) -> Result<Option<PathBuf>> {
    let out = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy()])
        .args(["worktree", "list", "--porcelain"])
        .output()
        .context("Failed to run git worktree list")?;
    anyhow::ensure!(
        out.status.success(),
        "git worktree list failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );

    let mut current_path = None;
    for line in String::from_utf8_lossy(&out.stdout).lines() {
        if let Some(path) = line.strip_prefix("worktree ") {
            current_path = Some(PathBuf::from(path.trim()));
            continue;
        }
        if line.trim().is_empty() {
            current_path = None;
            continue;
        }
        if let (Some(path), Some(branch_line)) =
            (current_path.as_ref(), line.strip_prefix("branch "))
        {
            let branch_name = branch_line
                .trim()
                .strip_prefix("refs/heads/")
                .unwrap_or(branch_line.trim());
            if branch_name == branch {
                return Ok(Some(path.clone()));
            }
        }
    }

    Ok(None)
}

pub fn local_branch_exists(repo_dir: &Path, branch: &str) -> Result<bool> {
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            &format!("refs/heads/{branch}"),
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git rev-parse")?;
    Ok(status.success())
}

pub fn branch_has_commits_ahead_of_main(repo_dir: &Path, branch: &str) -> Result<bool> {
    super::validate_git_repo(repo_dir)?;
    let status = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-parse",
            "--verify",
            branch,
        ])
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .context("Failed to run git")?;
    if !status.success() {
        return Ok(false);
    }

    let out = Command::new("git")
        .args([
            "-C",
            &repo_dir.to_string_lossy(),
            "rev-list",
            "--count",
            &format!("main..{branch}"),
        ])
        .output()
        .context("Failed to run git rev-list")?;
    if !out.status.success() {
        return Ok(false);
    }
    Ok(String::from_utf8_lossy(&out.stdout)
        .trim()
        .parse::<u32>()
        .unwrap_or(0)
        > 0)
}

pub fn worktree_changed_files(wt_path: &Path) -> Result<Vec<String>> {
    let repo = wt_path.to_string_lossy().to_string();
    let range = if commits_ahead_of_main(&repo).unwrap_or(0) > 1 {
        "main..HEAD"
    } else {
        "HEAD~1..HEAD"
    };
    let out = Command::new("git")
        .args(["-C", &repo, "diff", "--name-only", range])
        .output()
        .context("Failed to run git diff --name-only")?;
    anyhow::ensure!(
        out.status.success(),
        "git diff failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    let files = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|line| !line.is_empty())
        .map(ToString::to_string)
        .collect();
    Ok(files)
}

const LOCK_FILENAME: &str = ".aid-lock";
static LOCK_TEMP_COUNTER: AtomicU64 = AtomicU64::new(0);

pub fn check_worktree_lock(wt_path: &Path) -> Option<String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let content = std::fs::read_to_string(&lock_path).ok()?;
    let mut task_id = None;
    let mut pid = None;
    for line in content.lines() {
        if let Some(t) = line.strip_prefix("task=") {
            task_id = Some(t.trim().to_string());
        }
        if let Some(p) = line.strip_prefix("pid=") {
            pid = p.trim().parse::<u32>().ok();
        }
    }
    if let Some(p) = pid
        && !process_alive(p)
    {
        let _ = std::fs::remove_file(&lock_path);
        return None;
    }
    task_id
}

pub fn write_worktree_lock(wt_path: &Path, task_id: &str) {
    let lock_path = wt_path.join(LOCK_FILENAME);
    let content = format!("task={task_id}\npid={}\n", std::process::id());
    let _ = std::fs::write(&lock_path, content);
}

fn unique_lock_extension(prefix: &str) -> String {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_nanos())
        .unwrap_or(0);
    format!(
        "{prefix}.{}.{}.{}",
        std::process::id(),
        nanos,
        LOCK_TEMP_COUNTER.fetch_add(1, Ordering::Relaxed)
    )
}

pub fn try_acquire_worktree_lock(wt_path: &Path, task_id: &str) -> std::result::Result<(), String> {
    let lock_path = wt_path.join(LOCK_FILENAME);
    for attempt in 0..2 {
        let temp_path = lock_path.with_extension(unique_lock_extension("tmp"));
        let content = format!("task={task_id}\npid={}\n", std::process::id());
        let write_result = OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&temp_path)
            .and_then(|mut file| file.write_all(content.as_bytes()));
        if let Err(err) = write_result {
            let _ = std::fs::remove_file(&temp_path);
            return Err(err.to_string());
        }

        let link_result = std::fs::hard_link(&temp_path, &lock_path);
        let _ = std::fs::remove_file(&temp_path);
        match link_result {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
                let holder = check_worktree_lock(wt_path);
                if !lock_path.exists() && attempt == 0 {
                    continue;
                }
                if holder.is_none() && attempt == 0 {
                    // Empty or malformed locks predate atomic acquisition; take and remove once.
                    let cleanup_path = lock_path.with_extension(unique_lock_extension("malformed"));
                    match std::fs::rename(&lock_path, &cleanup_path) {
                        Ok(()) => {
                            let _ = std::fs::remove_file(&cleanup_path);
                            continue;
                        }
                        Err(err) if err.kind() == std::io::ErrorKind::NotFound => continue,
                        Err(err) => return Err(err.to_string()),
                    }
                }
                return Err(holder.unwrap_or_else(|| "unknown".to_string()));
            }
            Err(err) => return Err(err.to_string()),
        }
    }
    Err("unknown".to_string())
}

pub fn clear_worktree_lock(wt_path: &Path) {
    let _ = std::fs::remove_file(wt_path.join(LOCK_FILENAME));
    if let Ok(entries) = std::fs::read_dir(wt_path) {
        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str.starts_with(".aid-lock.tmp.")
                || name_str.starts_with(".aid-lock.malformed.")
            {
                let _ = std::fs::remove_file(entry.path());
            }
        }
    }
}

pub fn process_alive_check(pid: u32) -> bool {
    process_alive(pid)
}

#[cfg(unix)]
fn process_alive(pid: u32) -> bool {
    unsafe { libc::kill(pid as i32, 0) == 0 }
}

#[cfg(not(unix))]
fn process_alive(_pid: u32) -> bool {
    false
}

fn commits_ahead_of_main(repo: &str) -> Option<u32> {
    let out = Command::new("git")
        .args(["-C", repo, "rev-list", "--count", "main..HEAD"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let output = String::from_utf8_lossy(&out.stdout);
    let trimmed = output.trim();
    trimmed.parse::<u32>().ok()
}
