// Verify output capture and bounded rbox lock-queue accounting.
// Preserves stdout/stderr bytes while timing lock notices through the job-start marker.
// Deps: std pipe readers, synchronized clocks and verifier result handling.

use crate::remote_build::LockQueue;
use anyhow::{Context, Result};
use std::io::{BufRead, BufReader, Read};
use std::process::Child;
use std::sync::Arc;
use std::thread;

pub(super) fn spawn_output_reader(
    child: &mut Child,
    queue: Arc<LockQueue>,
) -> Result<thread::JoinHandle<Result<String>>> {
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
        Ok(format!("{stdout}{stderr}"))
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
