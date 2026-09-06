// Shared Git staging policy for aid-owned files and generated directories.
// Exports staging modes, exclusions, and the stage_aid_files helper.
// Deps: anyhow, std::path, std::process.

use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// `git add` pathspec exclusions for aid's own runtime bookkeeping/artifacts —
/// a target repo won't gitignore these itself, so any `git add` run by aid or
/// a dispatched agent must exclude them explicitly.
pub const AID_ADD_EXCLUDES: &[&str] = &[
    ":(exclude).aid-*",
    ":(exclude)**/.aid-*",
    ":(exclude)result-*.md",
    ":(exclude)result-*.json",
    ":(exclude)aid-batch-*",
    ":(exclude)**/aid-batch-*",
];

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum AidStageMode {
    All,
    Tracked,
}

pub(crate) fn stage_aid_files(
    dir: &Path,
    mode: AidStageMode,
    reset_paths: &[&str],
) -> Result<()> {
    let add_mode = match mode {
        AidStageMode::All => "-A",
        AidStageMode::Tracked => "-u",
    };
    let mut add = Command::new("git");
    add.arg("-C")
        .arg(dir)
        .args(["add", add_mode, "--", "."])
        .args(AID_ADD_EXCLUDES);
    let output = add.output().context("Failed to run git add")?;
    anyhow::ensure!(output.status.success(), "git add failed: {}", String::from_utf8_lossy(&output.stderr));

    let mut reset = Command::new("git");
    reset
        .arg("-C")
        .arg(dir)
        .args(["reset", "-q", "--", ".aid/state.toml", ".aid/batches"])
        .args(reset_paths);
    let reset = reset.output().context("Failed to run git reset")?;
    anyhow::ensure!(
        reset.status.success(),
        "git reset failed: {}",
        String::from_utf8_lossy(&reset.stderr)
    );
    Ok(())
}
