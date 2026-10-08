// PTY process bridge for interactive background agents.
// Spawns commands under a native PTY and exposes read/write child control handles.

use anyhow::{Context, Result};
use portable_pty::{native_pty_system, Child, CommandBuilder, ExitStatus, MasterPty, PtySize};
use std::io::{Read, Write};
use std::time::{Duration, Instant};

#[cfg(unix)]
const PTY_KILL_GRACE: Duration = Duration::from_secs(3);
/// Longest one input write may wait for room in the child's tty input queue.
/// A child that stops reading stdin leaves the queue full after ~1 KiB
/// on macOS; an unbounded write then blocks the monitor thread for the rest of the run.
const INPUT_WRITE_TIMEOUT: Duration = Duration::from_secs(2);
#[cfg(unix)]
const CLOSED_REVENTS: libc::c_short = libc::POLLHUP | libc::POLLERR | libc::POLLNVAL;

pub struct PtyBridge {
    master: Box<dyn MasterPty + Send>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
}

impl PtyBridge {
    pub fn spawn(cmd: &[String], dir: Option<&str>, env: Vec<(String, Option<String>)>) -> Result<Self> {
        let program = cmd.first().context("PTY command is missing a program")?;
        let pty = native_pty_system().openpty(PtySize {
            rows: 24,
            cols: 80,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut builder = CommandBuilder::new(program);
        for arg in cmd.iter().skip(1) {
            builder.arg(arg);
        }
        if let Some(dir) = dir {
            builder.cwd(dir);
        }
        for (key, value) in env {
            match value {
                Some(value) => builder.env(key, value),
                None => builder.env_remove(key),
            }
        }
        let reader = pty.master.try_clone_reader()?;
        let writer = pty.master.take_writer()?;
        let child = pty.slave.spawn_command(builder)?;
        Ok(Self {
            master: pty.master,
            reader: Some(reader),
            writer,
            child,
        })
    }

    pub fn take_reader(&mut self) -> Result<Box<dyn Read + Send>> {
        self.reader
            .take()
            .context("PTY reader has already been taken")
    }

    /// Writes one byte at a time, each only after poll reports room, so a child
    /// that stops reading input fails the delivery instead of wedging the caller.
    pub fn write_input(&mut self, input: &str) -> Result<()> {
        let deadline = Instant::now() + INPUT_WRITE_TIMEOUT;
        let newline = (!input.ends_with('\n')).then_some(b'\n');
        for byte in input.bytes().chain(newline) {
            self.wait_writable(deadline)?;
            self.writer.write_all(&[byte])?;
        }
        self.writer.flush()?;
        Ok(())
    }

    #[cfg(unix)]
    fn wait_writable(&self, deadline: Instant) -> Result<()> {
        let Some(fd) = self.master.as_raw_fd() else {
            return Ok(());
        };
        loop {
            let remaining = deadline.saturating_duration_since(Instant::now());
            let mut poll_fd = libc::pollfd { fd, events: libc::POLLOUT, revents: 0 };
            let timeout_ms = remaining.as_millis().min(i32::MAX as u128) as i32;
            match unsafe { libc::poll(&mut poll_fd, 1, timeout_ms) } {
                // A hung-up master can still block a write on macOS: refuse it here.
                ready if ready > 0 && poll_fd.revents & CLOSED_REVENTS != 0 => anyhow::bail!(
                    "PTY closed (poll revents={:#x})",
                    poll_fd.revents
                ),
                ready if ready > 0 && poll_fd.revents & libc::POLLOUT != 0 => return Ok(()),
                ready if ready > 0 => continue,
                0 => anyhow::bail!(
                    "agent is not reading terminal input (PTY input queue full for {}s)",
                    INPUT_WRITE_TIMEOUT.as_secs()
                ),
                _ => {
                    let error = std::io::Error::last_os_error();
                    if error.kind() != std::io::ErrorKind::Interrupted {
                        return Err(error.into());
                    }
                }
            }
        }
    }

    #[cfg(not(unix))]
    fn wait_writable(&self, _deadline: Instant) -> Result<()> {
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        self.child
            .try_wait()
            .map(|status| status.is_none())
            .unwrap_or(false)
    }

    pub fn kill(&mut self) -> Result<()> {
        self.child
            .kill()
            .map_err(|e| anyhow::anyhow!("PTY kill failed: {e}"))
    }

    pub fn child_pid(&self) -> Option<u32> {
        self.child.process_id()
    }

    /// Walks descendants only from a leader this bridge has not reaped: an unreaped
    /// child pins its pid, while a reaped one may already name another process.
    #[cfg(unix)]
    pub fn kill_group(&mut self) -> Result<()> {
        if let Some(pid) = self.child.process_id() {
            if self.is_alive() {
                crate::process_group::kill_with_grace(pid as i32, PTY_KILL_GRACE);
            } else {
                crate::process_group::kill_group_with_grace(pid as i32, PTY_KILL_GRACE);
            }
            return Ok(());
        }
        self.child
            .kill()
            .map_err(|e| anyhow::anyhow!("PTY kill failed: {e}"))
    }

    #[allow(dead_code)]
    pub fn try_wait(&mut self) -> Result<Option<ExitStatus>> {
        self.child
            .try_wait()
            .map_err(|e| anyhow::anyhow!("try_wait failed: {e}"))
    }

    pub fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait()?)
    }
}

#[cfg(test)]
mod tests {
    use super::PtyBridge;
    use crate::test_subprocess;

    #[test]
    fn spawns_echo_in_a_pty() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/echo".to_string(), "hello".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();
        let mut reader = bridge.take_reader().unwrap();

        let mut output = String::new();
        reader.read_to_string(&mut output).unwrap();
        let _ = bridge.wait().unwrap();
        assert!(output.contains("hello"));
    }

    #[test]
    fn kill_terminates_running_process() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sleep".to_string(), "60".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();

        assert!(bridge.is_alive());
        bridge.kill().unwrap();
        let _ = bridge.wait().unwrap();
        assert!(!bridge.is_alive());
    }

    #[test]
    fn try_wait_returns_none_while_running() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sleep".to_string(), "60".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();

        assert!(bridge.try_wait().unwrap().is_none());
        bridge.kill().unwrap();
        let _ = bridge.wait().unwrap();
        assert!(bridge.try_wait().unwrap().is_some());
    }

    #[test]
    fn child_pid_returns_some_while_running() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sleep".to_string(), "60".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();

        let pid = bridge.child_pid();
        assert!(pid.is_some());
        assert!(pid.unwrap() > 0);
        bridge.kill().unwrap();
        let _ = bridge.wait().unwrap();
    }

    #[cfg(unix)]
    #[test]
    fn kill_group_terminates_process() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sleep".to_string(), "60".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();

        assert!(bridge.is_alive());
        bridge.kill_group().unwrap();
        let _ = bridge.wait().unwrap();
        assert!(!bridge.is_alive());
    }

    /// An 853-byte steer fit the tty input queue of a child that never
    /// reads stdin; the next one blocked the PTY write forever. The second message is
    /// sized past the Linux limit too (4 KiB line buffer + 64 KiB flip buffer).
    #[test]
    fn write_input_to_child_that_never_reads_fails_instead_of_blocking() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sleep".to_string(), "60".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();
        let mut reader = bridge.take_reader().unwrap();
        std::thread::spawn(move || {
            let mut buffer = [0_u8; 4096];
            while matches!(reader.read(&mut buffer), Ok(size) if size > 0) {}
        });
        let first = "a".repeat(852);
        let second = format!("{}\n", "b".repeat(99)).repeat(1_400);
        let (tx, rx) = std::sync::mpsc::channel();
        let writer = std::thread::spawn(move || {
            let results = [bridge.write_input(&first).is_ok(), bridge.write_input(&second).is_ok()];
            let _ = tx.send(results);
            bridge
        });

        let results = rx.recv_timeout(std::time::Duration::from_secs(30));
        assert_eq!(results.ok(), Some([true, false]), "second write must fail, not block");
        let mut bridge = writer.join().unwrap();
        bridge.kill().unwrap();
        let _ = bridge.wait().unwrap();
    }

    /// The child exited and closed the last slave fd between `is_alive()` and the
    /// write: poll reports a hang-up, which must refuse the input before any write
    /// (a BSD pty master can block on it instead of returning EIO).
    #[cfg(unix)]
    #[test]
    fn write_input_after_child_exit_refuses_promptly() {
        let _permit = test_subprocess::acquire();
        let cmd = vec!["/bin/sh".to_string(), "-c".to_string(), "exit 0".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();
        let _ = bridge.wait().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let started = std::time::Instant::now();
            let error = bridge.write_input("late steer").err().map(|error| error.to_string());
            let _ = tx.send((error, started.elapsed()));
        });

        let (error, elapsed) = rx.recv_timeout(std::time::Duration::from_secs(10))
            .expect("write to a closed PTY must not block");
        let error = error.expect("write to a closed PTY must fail");
        assert!(error.contains("PTY closed"), "unexpected error: {error}");
        assert!(elapsed < std::time::Duration::from_secs(1), "refusal took {elapsed:?}");
    }
}
