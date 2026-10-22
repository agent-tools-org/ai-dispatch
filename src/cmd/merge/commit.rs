// Auto-commit preparation for aid merge.
// Deps: git CLI and worktree staging helpers.

use super::*;

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
    aid_info!("[aid] Worktree has uncommitted changes — auto-committing on {branch}");
    if let Err(error) = crate::workspace::worktree::stage_aid_files(
        Path::new(wt_path),
        crate::workspace::worktree::AidStageMode::All,
        &[
            "target/",
            "node_modules/",
            ".build/",
            "dist/",
            "__pycache__/",
        ],
    ) {
        aid_warn!("[aid] Warning: failed to stage worktree changes: {error}");
        return false;
    }
    if !has_staged_changes(wt_path) {
        return false;
    }
    let message = auto_commit_message(wt_path);
    commit_worktree_changes(wt_path, &message)
}

fn commit_worktree_changes(wt_path: &str, message: &str) -> bool {
    let out = Command::new("git")
        .args(["-C", wt_path, "commit", "-m", &message])
        .output();
    match out {
        Ok(o) if o.status.success() => {
            aid_info!("[aid] Auto-committed uncommitted changes");
            true
        }
        Ok(o) => {
            let stderr = String::from_utf8_lossy(&o.stderr);
            aid_warn!(
                "[aid] Warning: auto-commit failed: {}",
                stderr.lines().next().unwrap_or("")
            );
            false
        }
        Err(e) => {
            aid_warn!("[aid] Warning: auto-commit failed: {e}");
            false
        }
    }
}

fn has_staged_changes(dir: &str) -> bool {
    let out = Command::new("git")
        .args(["-C", dir, "diff", "--cached", "--quiet"])
        .output();
    match out {
        Ok(o) => o.status.code() == Some(1),
        _ => false,
    }
}

fn auto_commit_message(wt_path: &str) -> String {
    let diff_stat = Command::new("git")
        .args([
            "-C",
            wt_path,
            "diff",
            "--cached",
            "--stat",
            "--stat-width=60",
        ])
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default();
    let file_count = diff_stat.lines().count().saturating_sub(1);
    if file_count == 1 {
        let file = diff_stat
            .lines()
            .next()
            .and_then(|line| line.split('|').next())
            .map(str::trim)
            .unwrap_or("1 file");
        format!("chore: auto-commit changes to {file}")
    } else if file_count > 1 {
        format!("chore: auto-commit changes to {file_count} files")
    } else {
        "chore: auto-commit agent changes before merge".to_string()
    }
}
