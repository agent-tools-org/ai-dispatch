// Bounded JSON-RPC stdio transport with owned process-group cleanup.
// Reader and writer threads keep pipe backpressure outside worker deadline enforcement.

use anyhow::{Context, Result};
use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::process::{Child, Command, Stdio};
use std::sync::mpsc::{self, Receiver, SyncSender};
use std::time::Duration;

#[derive(Debug)]
pub(super) struct Closed;

impl std::fmt::Display for Closed {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str("Codex app-server closed stdout before turn/completed")
    }
}

impl std::error::Error for Closed {}

pub(super) const RESPONSE_TIMEOUT: Duration = Duration::from_secs(10);
type WriteRequest = (Vec<u8>, mpsc::Sender<std::io::Result<()>>);

pub(super) struct Rpc {
    child: Child,
    incoming: Receiver<Result<Value>>,
    outgoing: Option<SyncSender<WriteRequest>>,
    reaped: bool,
    next_id: u64,
}

impl Rpc {
    pub fn prepare(cmd: &mut Command, stderr: &std::path::Path) -> Result<()> {
        #[cfg(unix)]
        {
            use std::os::unix::process::CommandExt;
            cmd.process_group(0);
        }
        cmd.stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::from(std::fs::File::create(stderr)?));
        Ok(())
    }

    pub fn spawn(cmd: &mut Command) -> Result<Child> {
        cmd.spawn().context("Spawn Codex app-server")
    }

    pub fn attach(mut child: Child) -> Result<Self> {
        let stdout = child.stdout.take().context("Missing Codex stdout")?;
        let stdin = child.stdin.take().context("Missing Codex stdin")?;
        Ok(Self {
            child,
            incoming: reader(stdout),
            outgoing: Some(writer(stdin)),
            reaped: false,
            next_id: 1,
        })
    }

    pub fn pid(&self) -> u32 {
        self.child.id()
    }

    pub fn request(&mut self, method: &str, params: Value) -> Result<u64> {
        let id = self.next_id;
        self.next_id += 1;
        self.send(serde_json::json!({"id":id,"method":method,"params":params}))?;
        Ok(id)
    }

    pub fn send(&self, value: Value) -> Result<()> {
        let mut bytes = serde_json::to_vec(&value)?;
        bytes.push(b'\n');
        let (sent, received) = mpsc::channel();
        self.outgoing
            .as_ref()
            .context("Codex protocol input closed")?
            .try_send((bytes, sent))
            .context("Codex protocol writer unavailable")?;
        received
            .recv_timeout(RESPONSE_TIMEOUT)
            .context("Codex protocol write timed out")??;
        Ok(())
    }

    pub fn close_input(&mut self) {
        self.outgoing.take();
    }

    pub fn wait_until(&mut self, deadline: std::time::Instant) -> Result<()> {
        while std::time::Instant::now() < deadline {
            if let Some(status) = self.child.try_wait()? {
                self.reaped = true;
                anyhow::ensure!(status.success(), "Codex app-server exited with {status}");
                break;
            }
            std::thread::park_timeout(Duration::from_millis(10));
        }
        Ok(())
    }

    pub fn receive(&self, timeout: Duration) -> Result<Option<Value>> {
        match self.incoming.recv_timeout(timeout) {
            Ok(value) => value.map(Some),
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(None),
            Err(_) => Err(Closed.into()),
        }
    }
}

impl Drop for Rpc {
    fn drop(&mut self) {
        if self.reaped {
            return;
        }
        #[cfg(unix)]
        crate::runtime::process::group::kill_with_grace(
            self.child.id() as i32,
            Duration::from_millis(100),
        );
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

fn reader(stdout: std::process::ChildStdout) -> Receiver<Result<Value>> {
    let (sender, receiver) = mpsc::sync_channel(64);
    std::thread::spawn(move || {
        for line in BufReader::new(stdout).lines() {
            let value = line.map_err(anyhow::Error::from).and_then(|line| {
                serde_json::from_str(&line).context("Invalid Codex JSON-RPC message")
            });
            let failed = value.is_err();
            if sender.send(value).is_err() || failed {
                break;
            }
        }
    });
    receiver
}

fn writer(mut stdin: std::process::ChildStdin) -> SyncSender<WriteRequest> {
    let (sender, receiver) = mpsc::sync_channel::<WriteRequest>(1);
    std::thread::spawn(move || {
        while let Ok((bytes, sent)) = receiver.recv() {
            let result = stdin.write_all(&bytes).and_then(|()| stdin.flush());
            let failed = result.is_err();
            let _ = sent.send(result);
            if failed {
                break;
            }
        }
    });
    sender
}
