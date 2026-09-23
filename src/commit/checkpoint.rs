// Recovery checkpoints for tasks running in a principal's shared checkout.
// Exports preserve_shared_checkout; never changes HEAD, the real index or files.
// Deps: Git plumbing, private scratch index, existing aid staging exclusions.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

pub(crate) struct CheckoutCheckpoint {
    pub reference: String,
    pub commit: String,
}

/// This snapshots the checkout, not just task-owned edits: a shared directory
/// cannot reliably attribute concurrent edits. Restore individual files after
/// review; never automatically merge this recovery ref as the task's work.
pub(crate) fn preserve_shared_checkout(
    dir: &Path,
    task_id: &str,
) -> Result<Option<CheckoutCheckpoint>> {
    crate::sanitize::validate_task_id(task_id)?;
    let probe = Command::new("git")
        .arg("-C").arg(dir)
        .env("LC_ALL", "C")
        .args(["rev-parse", "--show-toplevel"])
        .output().context("Failed to inspect shared checkout")?;
    if !probe.status.success() {
        // Non-Git tasks have no branch or index to settle. Other Git failures
        // must remain visible rather than being mistaken for successful rescue.
        let error = String::from_utf8_lossy(&probe.stderr);
        if error.contains("not a git repository") {
            return Ok(None);
        }
        anyhow::bail!("Failed to inspect shared checkout: {}", error.trim());
    }
    let root = PathBuf::from(String::from_utf8(probe.stdout)?.trim());
    let scratch = ScratchIndex::new()?;
    let index = scratch.0.join("index");
    let head = git(&root, &index, &["rev-parse", "--verify", "HEAD"]);
    let parent = match head {
        Ok(head) => {
            git(&root, &index, &["read-tree", &head])?;
            Some(head)
        }
        Err(error) => {
            // Only an unborn branch permits an empty base. A corrupt existing
            // branch must not produce a misleading root recovery commit.
            let branch = git(&root, &index, &["symbolic-ref", "HEAD"])?;
            let exists = Command::new("git").arg("-C").arg(&root)
                .args(["show-ref", "--verify", "--quiet", &branch]).status()?;
            anyhow::ensure!(exists.code() == Some(1), "Cannot read shared checkout HEAD: {error}");
            git(&root, &index, &["read-tree", "--empty"])?;
            None
        }
    };
    let base_tree = git(&root, &index, &["write-tree"])?;
    crate::worktree::stage_aid_files_with_index(
        &root,
        crate::worktree::AidStageMode::All,
        &["target", "node_modules", "__pycache__"],
        Some(&index),
    )?;
    let tree = git(&root, &index, &["write-tree"])?;
    if tree == base_tree {
        return Ok(None);
    }
    let message = format!(
        "[aid] shared checkout recovery for {task_id}\n\nIncludes shared edits; not a task-owned merge artifact."
    );
    let mut args = vec!["commit-tree", tree.as_str(), "-m", message.as_str()];
    if let Some(parent) = parent.as_deref() {
        args.extend(["-p", parent]);
    }
    let commit = git(&root, &index, &args)?;
    // Each attempt owns a new ref. Never replace an earlier recovery point,
    // including concurrent settlements/retries using the same task ID.
    let reference = format!("refs/aid/recovery/{task_id}/{:032x}", rand::random::<u128>());
    git(&root, &index, &["update-ref", &reference, &commit, ""])?;
    Ok(Some(CheckoutCheckpoint { reference, commit }))
}

fn git(root: &Path, index: &Path, args: &[&str]) -> Result<String> {
    let output = Command::new("git")
        .arg("-C").arg(root)
        .args(["-c", "user.name=aid recovery", "-c", "user.email=aid@localhost"])
        .env("GIT_INDEX_FILE", index)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .args(args).output().context("Failed to run checkpoint Git command")?;
    anyhow::ensure!(output.status.success(), "git {}: {}", args[0], String::from_utf8_lossy(&output.stderr).trim());
    Ok(String::from_utf8(output.stdout)?.trim().to_string())
}

struct ScratchIndex(PathBuf);

impl ScratchIndex {
    fn new() -> Result<Self> {
        use std::os::unix::fs::DirBuilderExt;
        let path = std::env::temp_dir().join(format!("aid-recovery-{:032x}", rand::random::<u128>()));
        std::fs::DirBuilder::new().mode(0o700).create(&path)?;
        Ok(Self(path.canonicalize()?))
    }
}

impl Drop for ScratchIndex {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

#[cfg(test)]
#[path = "checkpoint_tests.rs"]
mod tests;
