// Bounded CLI probes and identity checks for ambiguous agent command names.
// Exports: run_bounded, identity_markers, binary_identity_matches, help helpers,
// first_matching_executable, identity_exists_on_path.
// Deps: std process pipes, reader threads, and a fixed probe deadline.

use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::mpsc::{self, Receiver};

use anyhow::{Context, Result, bail};
use super::env::CliCommandOutput;
use std::time::{Duration, Instant};

const OUTPUT_LIMIT: usize = 64 * 1024;
const PROBE_TIMEOUT: Duration = Duration::from_millis(500);
pub(crate) const DEFAULT_PROBE_TIMEOUT: Duration = Duration::from_secs(10);

/// Walk `$PATH` in order and return the first executable `<dir>/<name>` that
/// satisfies `matches` (called with the absolute path). Non-executables are
/// skipped without probing — bare names like `agent` are too generic to trust
/// the OS's first hit.
pub(crate) fn first_matching_executable(
    path_value: Option<&OsStr>,
    name: &str,
    mut matches: impl FnMut(&str) -> bool,
) -> Option<String> {
    let path_value = path_value?;
    for dir in std::env::split_paths(path_value) {
        let candidate = dir.join(name);
        if !is_executable_file(&candidate) {
            continue;
        }
        let Some(candidate_str) = candidate.to_str() else {
            continue;
        };
        if matches(candidate_str) {
            return Some(candidate_str.to_owned());
        }
    }
    None
}

pub(crate) fn identity_exists_on_path(name: &str, markers: &[&str]) -> bool {
    first_matching_executable(std::env::var_os("PATH").as_deref(), name, |path| {
        binary_identity_matches(path, markers)
    })
    .is_some()
}

fn is_executable_file(path: &Path) -> bool {
    let Ok(metadata) = fs::metadata(path) else {
        return false;
    };
    if !metadata.is_file() {
        return false;
    }
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        metadata.permissions().mode() & 0o111 != 0
    }
    #[cfg(not(unix))]
    {
        true
    }
}

pub(crate) fn binary_identity_matches(name: &str, markers: &[&str]) -> bool {
    run_bounded(name, &["--help"], PROBE_TIMEOUT).is_ok_and(|output| {
        output.success && help_identifies(&format!("{}{}", output.stdout, output.stderr), markers)
    })
}

pub(crate) fn run_bounded(program: &str, args: &[&str], timeout: Duration) -> Result<CliCommandOutput> {
    let deadline = Instant::now() + timeout;
    let mut child = Command::new(program).args(args)
        .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::piped())
        .spawn().with_context(|| format!("failed to probe {program}"))?;
    let stdout = child.stdout.take().map(capped_reader);
    let stderr = child.stderr.take().map(capped_reader);
    let result = (|| {
        let status = loop {
            match child.try_wait()? {
                Some(status) => break status,
                None if Instant::now() >= deadline => bail!("{program} probe timed out"),
                None => std::thread::sleep(Duration::from_millis(5)),
            }
        };
        Ok(CliCommandOutput {
            success: status.success(),
            stdout: collect_output(stdout, deadline)?,
            stderr: collect_output(stderr, deadline)?,
        })
    })();
    if result.is_err() {
        let _ = child.kill();
        let _ = child.wait();
    }
    result
}

fn capped_reader<R: Read + Send + 'static>(mut stream: R) -> Receiver<Vec<u8>> {
    let (tx, rx) = mpsc::channel();
    std::thread::spawn(move || {
        let mut output = Vec::new();
        let mut buffer = [0; 8192];
        loop {
            match stream.read(&mut buffer) {
                Ok(0) => break,
                Ok(n) => {
                    let keep = n.min(OUTPUT_LIMIT - output.len());
                    output.extend_from_slice(&buffer[..keep]);
                }
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => break,
            }
        }
        let _ = tx.send(output);
    });
    rx
}

fn collect_output(reader: Option<Receiver<Vec<u8>>>, deadline: Instant) -> Result<String> {
    let Some(reader) = reader else { return Ok(String::new()); };
    let output = reader.recv_timeout(deadline.saturating_duration_since(Instant::now()))
        .context("CLI probe output timed out")?;
    Ok(String::from_utf8_lossy(&output).into_owned())
}

/// True when the help text names the product through any of `markers` (case-insensitive).
pub(crate) fn help_identifies(help: &str, markers: &[&str]) -> bool {
    let help = help.to_ascii_lowercase();
    markers.iter().any(|marker| help.contains(marker))
}

/// True when help defines a flag, excluding prefixes and mentions in prose.
pub(crate) fn help_defines_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        line.trim_start().strip_prefix(flag).is_some_and(|rest| {
            rest.is_empty() || rest.starts_with([' ', '\t', '=', ','])
        })
    })
}

/// Product names a generic command name must print in `--help` to count as that agent.
/// `agent` needs Cursor's product name, not a bare "cursor": xAI's Grok Build CLI also
/// installs `agent` and its help lists a `cursor-worker` subcommand.
pub(crate) fn identity_markers(name: &str) -> Option<&'static [&'static str]> {
    match name {
        "agent" => Some(&["cursor agent", "cursor-agent"]),
        "claude" => Some(&["claude code"]),
        "oz" => Some(&["warp"]),
        _ => None,
    }
}

#[cfg(test)]
#[path = "env_identity_tests.rs"]
mod tests;
