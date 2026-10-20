// Deadline-bounded replacement selection for remote verification.
// Exports pick_before; uses ProcessGuard and temporary output files to avoid pipe stalls.
use super::*;
use crate::runtime::process::guard::ProcessGuard;
use std::io::{Read, Seek};

pub(super) fn pick_before(
    command: &mut Command,
    path: &Path,
    failed: &str,
    deadline: chrono::DateTime<chrono::Local>,
) -> Result<String> {
    let repo =
        crate::config::project::worktree::main_working_tree(path).unwrap_or_else(|| path.to_path_buf());
    command
        .args(["pick", "--tool", "rust", "--repo"])
        .arg(repo)
        .args(["--exclude", failed]);
    let mut stdout = tempfile::tempfile()?;
    let mut stderr = tempfile::tempfile()?;
    command.stdout(stdout.try_clone()?).stderr(stderr.try_clone()?);
    let remaining = remaining_verify_time(deadline, chrono::Local::now());
    if remaining.is_zero() {
        bail!("Task deadline exhausted before re-pick");
    }
    let mut guard = ProcessGuard::spawn(command)?;
    let status = guard
        .wait_with_timeout(remaining)?
        .context("Task deadline exhausted during re-pick")?;
    stdout.rewind()?;
    stderr.rewind()?;
    let mut name = String::new();
    let mut error = String::new();
    stdout.read_to_string(&mut name)?;
    stderr.read_to_string(&mut error)?;
    if !status.success() {
        bail!("Remote build box selection failed: {}", error.trim());
    }
    let name = name.trim();
    if name.is_empty() {
        bail!("rbox pick returned an empty build box name");
    }
    if name == failed {
        bail!("rbox pick returned the excluded build box {failed}");
    }
    Ok(name.to_string())
}
