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

    #[allow(dead_code)]
    pub fn reader(&mut self) -> &mut dyn Read {
        self.reader
            .as_deref_mut()
            .expect("PTY reader has already been taken")
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

    pub fn wait(&mut self) -> Result<ExitStatus> {
        Ok(self.child.wait()?)
    }
}

#[cfg(test)]
mod tests {
    use super::PtyBridge;

    #[test]
    fn spawns_echo_in_a_pty() {
        let cmd = vec!["/bin/echo".to_string(), "hello".to_string()];
        let mut bridge = PtyBridge::spawn(&cmd, None, vec![]).unwrap();

        let mut output = String::new();
        bridge.reader().read_to_string(&mut output).unwrap();
        let _ = bridge.wait().unwrap();
        assert!(output.contains("hello"));
    }
}
