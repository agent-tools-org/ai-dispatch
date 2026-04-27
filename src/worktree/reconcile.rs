// Worktree reconciliation helpers for stale reused worktrees.
// Exports: maybe_refresh_existing_worktree for safe behind-HEAD refresh.
// Deps: git CLI via std::process::Command, anyhow.
use anyhow::{Context, Result, anyhow};
use std::path::Path;
use std::process::{Command, Stdio};

pub(super) fn maybe_refresh_existing_worktree(
    repo_dir: &Path,
    wt_path: &Path,
    branch: &str,
    base_branch: Option<&str>,
) -> Result<()> {
    // Issue #113: an agent (or operator) running `git checkout <other>` inside
    // an aid-managed worktree silently steers subsequent dispatches onto the
    // wrong branch. Their commits then never advance the requested branch ref
    // and become invisible to the main repo. Re-anchor the worktree to the
    // expected branch before any other reconciliation.
    ensure_worktree_on_branch(wt_path, branch)?;

    let target_ref = base_branch.unwrap_or("HEAD");
    let repo_head = rev_parse(repo_dir, target_ref)?;
    let worktree_head = rev_parse(wt_path, "HEAD")?;
    if repo_head == worktree_head {
        return Ok(());
    }

    let missing_commits = rev_list_count(repo_dir, &format!("{worktree_head}..{repo_head}"))?;
    if missing_commits == 0 {
        return Ok(());
    }

    let unique_commits = rev_list_count(repo_dir, &format!("{repo_head}..{worktree_head}"))?;
    if unique_commits > 0 {
        return Err(stale_worktree_error(
            wt_path,
            branch,
            format!(
                "it has {unique_commits} commit(s) not on the current repo HEAD"
            ),
        ));
    }

    if has_uncommitted_changes(wt_path)? {
        return Err(stale_worktree_error(
            wt_path,
            branch,
            "it has uncommitted changes".to_string(),
        ));
    }

    let status = Command::new("git")
        .args(["-C", &wt_path.to_string_lossy(), "reset", "--hard", &repo_head])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to refresh stale worktree")?;
    anyhow::ensure!(
        status.success(),
        "{}",
        stale_worktree_error(
            wt_path,
            branch,
            "git reset --hard failed while refreshing".to_string(),
        )
    );
    aid_info!(
        "[aid] Refreshed stale worktree {} to current repo HEAD",
        wt_path.display()
    );
    Ok(())
}

fn rev_parse(repo_dir: &Path, rev: &str) -> Result<String> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", rev])
        .output()
        .with_context(|| format!("Failed to resolve git revision {rev}"))?;
    anyhow::ensure!(
        output.status.success(),
        "Failed to resolve git revision {rev}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

fn rev_list_count(repo_dir: &Path, range: &str) -> Result<u32> {
    let output = Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-list", "--count", range])
        .output()
        .with_context(|| format!("Failed to inspect git history for {range}"))?;
    anyhow::ensure!(
        output.status.success(),
        "Failed to inspect git history for {range}: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<u32>()
        .map_err(|err| anyhow!("Failed to parse git rev-list output for {range}: {err}"))
}

fn has_uncommitted_changes(wt_path: &Path) -> Result<bool> {
    let output = Command::new("git")
        .args(["-C", &wt_path.to_string_lossy(), "status", "--porcelain"])
        .output()
        .context("Failed to inspect worktree status")?;
    anyhow::ensure!(
        output.status.success(),
        "Failed to inspect worktree status: {}",
        String::from_utf8_lossy(&output.stderr).trim()
    );
    Ok(!String::from_utf8_lossy(&output.stdout).trim().is_empty())
}

fn stale_worktree_error(wt_path: &Path, branch: &str, reason: String) -> anyhow::Error {
    anyhow!(
        "Worktree {} is stale and cannot be auto-refreshed because {}. Run: aid worktree remove {}",
        wt_path.display(),
        reason,
        branch
    )
}

fn ensure_worktree_on_branch(wt_path: &Path, branch: &str) -> Result<()> {
    let current = current_branch(wt_path)?;
    if current.as_deref() == Some(branch) {
        return Ok(());
    }
    let observed = current.as_deref().unwrap_or("(detached HEAD)");
    if has_uncommitted_changes(wt_path)? {
        return Err(stale_worktree_error(
            wt_path,
            branch,
            format!(
                "HEAD is on {observed} (expected {branch}) and the worktree has uncommitted changes"
            ),
        ));
    }
    let status = Command::new("git")
        .args(["-C", &wt_path.to_string_lossy(), "checkout", branch])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .context("Failed to switch worktree to expected branch")?;
    if !status.success() {
        return Err(stale_worktree_error(
            wt_path,
            branch,
            format!(
                "HEAD is on {observed} (expected {branch}); auto-checkout to {branch} failed — branch may not exist"
            ),
        ));
    }
    aid_info!(
        "[aid] Re-anchored worktree {} from {observed} to {branch}",
        wt_path.display()
    );
    Ok(())
}

fn current_branch(wt_path: &Path) -> Result<Option<String>> {
    let output = Command::new("git")
        .args([
            "-C",
            &wt_path.to_string_lossy(),
            "symbolic-ref",
            "--short",
            "-q",
            "HEAD",
        ])
        .output()
        .context("Failed to inspect worktree branch")?;
    if !output.status.success() {
        return Ok(None);
    }
    let raw = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if raw.is_empty() {
        Ok(None)
    } else {
        Ok(Some(raw))
    }
}
