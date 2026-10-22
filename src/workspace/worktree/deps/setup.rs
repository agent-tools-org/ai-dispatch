// Setup process output capture and polling.
// Uses the parent event recorder and process guard to preserve setup ordering.

use super::*;

fn spawn_setup_reader<R: std::io::Read + Send + 'static>(
    reader: R,
    tx: mpsc::Sender<Option<String>>,
) -> std::thread::JoinHandle<Result<()>> {
    std::thread::spawn(move || {
        for line in BufReader::new(reader).lines() {
            let line = line?;
            let trimmed = line.trim();
            if !trimmed.is_empty() {
                let _ = tx.send(Some(trimmed.to_string()));
            }
        }
        Ok(())
    })
}

pub(super) fn join_setup_reader(handle: std::thread::JoinHandle<Result<()>>) -> Result<()> {
    handle
        .join()
        .map_err(|_| anyhow::anyhow!("setup output reader thread panicked"))?
}

pub(super) fn poll_setup(
    store: &Store,
    task_id: &TaskId,
    guard: &mut ProcessGuard,
    rx: &mpsc::Receiver<Option<String>>,
    timeout: Duration,
) -> Result<Option<std::process::ExitStatus>> {
    let start = Instant::now();
    let status = loop {
        while let Ok(message) = rx.try_recv() {
            if let Some(line) = message {
                insert_setup_event(store, task_id, &line);
            }
        }
        if let Some(status) = guard
            .child_mut()
            .try_wait()
            .context("Failed to poll setup command")?
        {
            break Some(status);
        }
        if start.elapsed() >= timeout {
            guard.force_kill();
            break None;
        }
        std::thread::sleep(Duration::from_millis(50));
    };
    Ok(status)
}

pub(super) fn capture_setup_output(
    guard: &mut ProcessGuard,
) -> Result<(
    mpsc::Receiver<Option<String>>,
    std::thread::JoinHandle<Result<()>>,
    std::thread::JoinHandle<Result<()>>,
)> {
    let stdout = guard
        .child_mut()
        .stdout
        .take()
        .context("setup stdout pipe missing")?;
    let stderr = guard
        .child_mut()
        .stderr
        .take()
        .context("setup stderr pipe missing")?;
    let (tx, rx) = mpsc::channel::<Option<String>>();
    let stdout_thread = spawn_setup_reader(stdout, tx.clone());
    let stderr_thread = spawn_setup_reader(stderr, tx);
    Ok((rx, stdout_thread, stderr_thread))
}
