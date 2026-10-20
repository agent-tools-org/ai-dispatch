// Deadline-bounded replacement selection with concurrently drained output pipes.
// Exports pick_before; uses ProcessGuard and standard-library reader threads.
use super::*;
use crate::runtime::process::guard::ProcessGuard;
use std::io::Read;
use std::process::Stdio;
use std::thread::{self, JoinHandle};

pub(super) fn pick_before(
    command: &mut Command,
    path: &Path,
    failed: &str,
    deadline: chrono::DateTime<chrono::Local>,
) -> Result<String> {
    let repo = crate::config::project::worktree::main_working_tree(path)
        .unwrap_or_else(|| path.to_path_buf());
    command
        .args(["pick", "--tool", "rust", "--repo"])
        .arg(repo)
        .args(["--exclude", failed]);
    command.stdout(Stdio::piped()).stderr(Stdio::piped());
    let remaining = remaining_verify_time(deadline, chrono::Local::now());
    if remaining.is_zero() {
        bail!("Task deadline exhausted before re-pick");
    }
    let mut guard = ProcessGuard::spawn(command)?;
    let child = guard.child_mut();
    let stdout = read_pipe(child.stdout.take().context("Missing rbox stdout pipe")?);
    let stderr = read_pipe(child.stderr.take().context("Missing rbox stderr pipe")?);
    let status = guard
        .wait_with_timeout(remaining)?
        .context("Task deadline exhausted during re-pick")?;
    let name = stdout
        .join()
        .map_err(|_| anyhow::anyhow!("rbox stdout reader panicked"))??;
    let error = stderr
        .join()
        .map_err(|_| anyhow::anyhow!("rbox stderr reader panicked"))??;
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

fn read_pipe(mut pipe: impl Read + Send + 'static) -> JoinHandle<std::io::Result<String>> {
    thread::spawn(move || {
        let mut output = String::new();
        pipe.read_to_string(&mut output)?;
        Ok(output)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pick_drains_both_pipes_before_waiting() {
        let _permit = crate::support::test_subprocess::acquire();
        let temp = tempfile::tempdir().expect("temp");
        let mut command = Command::new("bash");
        command.args([
            "-c",
            "printf 'second-box'; for ((i=0; i<16384; i++)); do printf 'diagnostic\\n' >&2; done",
        ]);
        let result = pick_before(
            &mut command,
            temp.path(),
            "failed-box",
            chrono::Local::now() + chrono::Duration::seconds(5),
        );
        assert_eq!(result.expect("pipes drained without timeout"), "second-box");
    }
}
