// Flush notifications and persistent session state before process teardown.
// Close input, drain bounded output, then let the process guard enforce cleanup.

use super::*;

impl Turn<'_> {
    pub(super) fn flush_terminal(&mut self) -> Result<()> {
        self.rpc.close_input();
        let grace = if self.sink.info.status == TaskStatus::Stopped {
            Duration::from_millis(500)
        } else {
            Duration::from_secs(2)
        };
        let deadline = Instant::now() + grace;
        while Instant::now() < deadline {
            match self.rpc.receive(Duration::from_millis(50)) {
                Ok(Some(message)) => self.notification(message)?,
                Ok(None) => {}
                Err(error) if error.is::<super::super::io::Closed>() => break,
                Err(error) => return Err(error),
            }
        }
        if let Some(terminal) = self.terminal.take() {
            for event in self.events.notification(&terminal) {
                self.sink.event(event)?;
            }
        }
        self.rpc.wait_until(deadline)?;
        Ok(())
    }
}

#[cfg(test)]
#[path = "finish_tests.rs"]
mod tests;
