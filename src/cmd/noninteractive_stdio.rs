// Non-interactive child stdio helpers for agent CLI execution.
// Exports command preparation that closes stdin and pipes stdout/stderr.

use std::process::Stdio;

use tokio::process::Command;

pub(crate) fn configure(cmd: &mut Command) {
    cmd.stdin(Stdio::null());
    cmd.stdout(Stdio::piped());
    cmd.stderr(Stdio::piped());
}

#[cfg(test)]
mod tests {
    use super::configure;
    use std::os::fd::OwnedFd;
    use std::os::unix::net::UnixStream;
    use tokio::process::Command;
    use tokio::runtime::Runtime;
    use tokio::time::{timeout, Duration};

    #[test]
    fn configure_overrides_blocking_stdin_with_null() {
        let runtime = Runtime::new().unwrap();
        runtime.block_on(async {
            let (reader, _writer) = UnixStream::pair().unwrap();
            let reader_fd: OwnedFd = reader.into();
            let mut cmd = Command::new("sh");
            cmd.args(["-c", "cat >/dev/null; printf done"]);
            cmd.stdin(std::process::Stdio::from(reader_fd));
            configure(&mut cmd);

            let output = timeout(Duration::from_secs(2), cmd.output())
                .await
                .expect("command timed out")
                .expect("command failed");

            assert!(output.status.success());
            assert_eq!(String::from_utf8_lossy(&output.stdout), "done");
        });
    }
}
