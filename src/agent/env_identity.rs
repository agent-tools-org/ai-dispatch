// Bounded identity probes for ambiguous agent command names.
// Exports: identity_marker and binary_identity_matches.
// Deps: std process pipes, reader threads, and a fixed probe deadline.

use std::io::Read;
use std::process::{Command, Stdio};
use std::thread::JoinHandle;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: u64 = 64 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);

pub(crate) fn binary_identity_matches(name: &str, marker: &str) -> bool {
    let mut command = Command::new(name);
    command.arg("--help").stdout(Stdio::piped()).stderr(Stdio::piped());
    let Ok(mut child) = command.spawn() else {
        return false;
    };
    let stdout = child.stdout.take().map(|stream| std::thread::spawn(move || read_capped(stream)));
    let stderr = child.stderr.take().map(|stream| std::thread::spawn(move || read_capped(stream)));
    let deadline = Instant::now() + PROBE_TIMEOUT;
    loop {
        match child.try_wait() {
            Ok(Some(status)) => {
                let stdout = join_output(stdout);
                let stderr = join_output(stderr);
                let text = format!(
                    "{}{}",
                    String::from_utf8_lossy(&stdout),
                    String::from_utf8_lossy(&stderr)
                );
                return status.success() && text.to_ascii_lowercase().contains(marker);
            }
            Ok(None) if Instant::now() >= deadline => {
                terminate_child(child);
                drop((stdout, stderr));
                return false;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(5)),
            Err(_) => {
                terminate_child(child);
                drop((stdout, stderr));
                return false;
            }
        }
    }
}

fn read_capped<R: Read>(stream: R) -> Vec<u8> {
    let mut output = Vec::new();
    let _ = stream.take(OUTPUT_LIMIT).read_to_end(&mut output);
    output
}

fn join_output(reader: Option<JoinHandle<Vec<u8>>>) -> Vec<u8> {
    reader.and_then(|handle| handle.join().ok()).unwrap_or_default()
}

fn terminate_child(mut child: std::process::Child) {
    let _ = std::thread::spawn(move || {
        let _ = child.kill();
        let _ = child.wait();
    });
}

pub(crate) fn identity_marker(name: &str) -> Option<&'static str> {
    match name {
        "agent" => Some("cursor"),
        "claude" => Some("claude code"),
        "oz" => Some("warp"),
        _ => None,
    }
}
