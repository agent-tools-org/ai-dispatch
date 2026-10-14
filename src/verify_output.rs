// Verify output capture and bounded rbox lock-queue accounting.
// Preserves stdout/stderr bytes while timing lock notices through the job-start marker.
// Deps: std pipe readers, synchronized clocks and verifier result handling.

use crate::remote_build::LockQueue;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::Arc;
use std::thread;

pub(super) struct CapturedOutput {
    pub(super) combined: String,
    stderr: String,
}

impl CapturedOutput {
    pub(super) fn shell_execution_failure(&self, exit_code: Option<i32>) -> bool {
        self.stderr.lines().any(|line| {
            let Some(diagnostic) = line.strip_prefix("sh: ") else { return false };
            match exit_code {
                Some(127) => diagnostic.ends_with(": not found")
                    || diagnostic.ends_with(": command not found"),
                Some(126) => diagnostic.ends_with(": Permission denied")
                    || diagnostic.ends_with(": cannot execute")
                    || diagnostic.ends_with(": Exec format error")
                    || diagnostic.contains(": cannot execute binary file"),
                _ => false,
            }
        })
    }
}

pub(super) fn spawn_output_reader(
    child: &mut Child,
    queue: Arc<LockQueue>,
) -> Result<thread::JoinHandle<Result<CapturedOutput>>> {
    let stdout = child.stdout.take().context("verify stdout pipe missing")?;
    let stderr = child.stderr.take().context("verify stderr pipe missing")?;
    Ok(thread::spawn(move || {
        let stdout_handle = thread::spawn(move || read_pipe(stdout, None));
        let stderr_handle = thread::spawn(move || read_pipe(stderr, Some(queue)));
        let stdout = stdout_handle
            .join()
            .map_err(|_| anyhow::anyhow!("verify stdout reader thread panicked"))??;
        let stderr = stderr_handle
            .join()
            .map_err(|_| anyhow::anyhow!("verify stderr reader thread panicked"))??;
        Ok(CapturedOutput { combined: format!("{stdout}{stderr}"), stderr })
    }))
}

fn read_pipe<R: Read>(reader: R, queue: Option<Arc<LockQueue>>) -> Result<String> {
    let mut reader = BufReader::new(reader);
    let mut bytes = Vec::new();
    let mut line = Vec::new();
    while reader.read_until(b'\n', &mut line)? > 0 {
        if let Some(queue) = queue.as_ref() {
            queue.observe(&String::from_utf8_lossy(&line));
        }
        bytes.extend_from_slice(&line);
        line.clear();
    }
    Ok(String::from_utf8_lossy(&bytes).into_owned())
}

#[cfg(test)]
mod tests {
    use super::CapturedOutput;

    #[test]
    fn shell_diagnostic_requires_matching_exit_code() {
        for (stderr, code) in [
            ("sh: 1: missing: not found", 127),
            ("sh: line 1: missing: command not found", 127),
            ("sh: 1: ./checker: Permission denied", 126),
            ("sh: ./checker: cannot execute binary file: Exec format error", 126),
        ] {
            let output = CapturedOutput { combined: stderr.into(), stderr: stderr.into() };
            assert!(output.shell_execution_failure(Some(code)), "{stderr}");
            assert!(!output.shell_execution_failure(Some(1)), "{stderr}");
            assert!(!output.shell_execution_failure(None), "{stderr}");
        }
    }

    #[test]
    fn checker_output_is_not_shell_execution_evidence() {
        let stdout = CapturedOutput {
            combined: "sh: 1: missing: not found".into(), stderr: String::new(),
        };
        assert!(!stdout.shell_execution_failure(Some(127)));
        let stderr = CapturedOutput {
            combined: "checker: input not found".into(), stderr: "checker: input not found".into(),
        };
        assert!(!stderr.shell_execution_failure(Some(127)));
        assert!(!stderr.shell_execution_failure(Some(126)));
    }
}
