// PTY process bridge for interactive background agents.
// Spawns commands under a native PTY and exposes read/write child control handles.

use anyhow::{Context, Result};
use portable_pty::{Child, CommandBuilder, ExitStatus, MasterPty, PtySize, native_pty_system};
use std::io::{Read, Write};

pub struct PtyBridge {
    _master: Box<dyn MasterPty + Send>,
    reader: Option<Box<dyn Read + Send>>,
    writer: Box<dyn Write + Send>,
    child: Box<dyn Child + Send>,
}

impl PtyBridge {
    pub fn spawn(cmd: &[String], dir: Option<&str>, env: Vec<(String, String)>) -> Result<Self> {
        let program = cmd
            .first()
            .context("PTY command is missing a program")?;
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
            builder.env(key, value);
        }
        let reader = pty.master.try_clone_reader()?;
        let writer = pty.master.take_writer()?;
        let child = pty.slave.spawn_command(builder)?;
        Ok(Self {
            _master: pty.master,
            reader: Some(reader),
            writer,
            child,
        })
    }

    pub fn take_reader(&mut self) -> Result<Box<dyn Read + Send>> {
        self.reader.take().context("PTY reader has already been taken")
    }

    pub fn write_input(&mut self, input: &str) -> Result<()> {
        self.writer.write_all(input.as_bytes())?;
        if !input.ends_with('\n') {
            self.writer.write_all(b"\n")?;
        }
        self.writer.flush()?;
        Ok(())
    }

    pub fn is_alive(&mut self) -> bool {
        self.child.try_wait().map(|status| status.is_none()).unwrap_or(false)
    }

    pub fn kill(&mut self) -> Result<()> {
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
}
