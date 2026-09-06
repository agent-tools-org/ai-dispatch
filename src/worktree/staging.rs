// Shared Git staging policy for aid-owned files and generated directories.
// Exports staging modes and the stage_aid_files helper.
// Deps: anyhow, std::path, std::process.

use anyhow::{Context, Result};
use std::path::Path;
use std::io::Write;
use std::process::{Command, Stdio};

/// `git add` pathspec exclusions for aid's own runtime bookkeeping/artifacts —
/// a target repo won't gitignore these itself, so any `git add` run by aid or
/// a dispatched agent must exclude them explicitly.
const AID_ADD_EXCLUDES: &[&str] = &[
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
    additional_candidates: &[&str],
) -> Result<()> {
    let add_mode = match mode {
        AidStageMode::All => "-A",
        AidStageMode::Tracked => "-u",
    };
    let mut candidates = vec![".aid/state.toml", ".aid/batches"];
    candidates.extend_from_slice(additional_candidates);
    let ignored = ignored_candidates(dir, &candidates)?;
    let mut excludes = candidates
        .iter()
        .filter(|candidate| !ignored.iter().any(|path| path == **candidate))
        .map(|candidate| format!(":(exclude){candidate}"))
        .collect::<Vec<_>>();
    excludes.extend(AID_ADD_EXCLUDES.iter().map(|path| (*path).to_string()));

    let mut add = Command::new("git");
    add.arg("-C")
        .arg(dir)
        .args(["add", add_mode, "--", "."])
        .args(&excludes);
    let output = add.output().context("Failed to run git add")?;
    anyhow::ensure!(output.status.success(), "git add failed: {}", String::from_utf8_lossy(&output.stderr));

    Ok(())
}

fn ignored_candidates(dir: &Path, candidates: &[&str]) -> Result<Vec<String>> {
    let mut check = Command::new("git")
        .arg("-C")
        .arg(dir)
        .args(["check-ignore", "--stdin"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .spawn()
        .context("Failed to run git check-ignore")?;
    if let Some(stdin) = check.stdin.as_mut() {
        stdin
            .write_all(candidates.join("\n").as_bytes())
            .context("Failed to provide paths to git check-ignore")?;
    }
    drop(check.stdin.take());
    let output = check
        .wait_with_output()
        .context("Failed to read git check-ignore")?;
    match output.status.code() {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(str::to_owned)
            .collect()),
        Some(1) => Ok(Vec::new()),
        _ => anyhow::bail!(
            "git check-ignore failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ),
    }
}
