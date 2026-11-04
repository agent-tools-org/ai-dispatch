// AIC cross-audit integration helpers.
// Exports: availability detection, timeout parsing, and `aic audit` execution.
// Deps: std::{env, io, process, thread, time}.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const PROBE_TIMEOUT_MS: u64 = 10_000;
const DEFAULT_AUDIT_TIMEOUT_SECS: u64 = 300;
const MAX_AUDIT_TIMEOUT_SECS: u64 = 1_800;

#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::{Mutex, OnceLock};
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    match LOCK.get_or_init(|| Mutex::new(())).lock() {
        Ok(guard) => guard,
        Err(poison) => poison.into_inner(),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditResult {
    pub verdict: String,
    pub report_path: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AicStatus {
    Missing,
    Foreign,
    TooOld(String),
    Ready,
}

impl AicStatus {
    pub fn diagnostic(&self) -> String {
        let description = match self {
            Self::Missing => "Missing: aic binary not found or aic --version failed".to_string(),
            Self::Foreign => "Foreign: aic --help does not identify the cross-auditor; \
                cargo install aic installs an unrelated project".to_string(),
            Self::TooOld(version) => format!("TooOld({version}): requires aic 0.3.2 or newer"),
            Self::Ready => "Ready".to_string(),
        };
        format!("{description}; install: curl -fsSL https://aid.agent-tools.org/install.sh | sh")
    }
}

pub fn status() -> AicStatus {
    detect_status()
}

pub fn is_available() -> bool {
    status() == AicStatus::Ready
}

pub fn audit_timeout_secs() -> u64 {
    std::env::var("AID_AUDIT_TIMEOUT_SECS")
        .ok()
        .and_then(|value| value.trim().parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .unwrap_or(DEFAULT_AUDIT_TIMEOUT_SECS)
        .min(MAX_AUDIT_TIMEOUT_SECS)
}

pub fn run_audit(task_id: &str, current_dir: Option<&Path>) -> AuditResult {
    let timeout_secs = audit_timeout_secs();
    let mut command = Command::new(aic_binary_path());
    command
        .args(["audit", task_id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }

    let child = match command.spawn() {
        Ok(child) => child,
        Err(err) => {
            return AuditResult {
                verdict: "error".to_string(),
                report_path: None,
                stdout: String::new(),
                stderr: err.to_string(),
                exit_code: None,
            };
        }
    };
    collect_audit_result(child, timeout_secs)
}

fn detect_status() -> AicStatus {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AIC_TEST_PRESENT") {
        return if matches!(value.trim().to_ascii_lowercase().as_str(), "1" | "true" | "yes") {
            AicStatus::Ready
        } else {
            AicStatus::Missing
        };
    }
    let Some((success, version)) = probe("--version") else {
        return AicStatus::Missing;
    };
    if !success {
        return AicStatus::Missing;
    }
    if !probe("--help").is_some_and(|(_, help)| help
        .lines().any(|line| line == "Cross-audit aid agent outputs"))
    {
        return AicStatus::Foreign;
    }
    let version = version.trim().to_string();
    if !supported_version(&version) {
        return AicStatus::TooOld(version);
    }
    AicStatus::Ready
}

fn probe_timeout() -> Duration {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AIC_TEST_PROBE_TIMEOUT_MS") {
        if let Ok(millis) = value.parse::<u64>() {
            return Duration::from_millis(millis.clamp(1, PROBE_TIMEOUT_MS));
        }
    }
    Duration::from_millis(PROBE_TIMEOUT_MS)
}

fn probe(flag: &str) -> Option<(bool, String)> {
    let mut child = Command::new(aic_binary_path()).arg(flag)
        .stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
    let stdout = child.stdout.take().map(read_pipe);
    let deadline = Instant::now() + probe_timeout();
    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(status)) if stdout.as_ref().is_some_and(|reader| reader.is_finished()) => {
                return Some((status.success(), join_reader(stdout)));
            }
            Ok(_) => std::thread::sleep(Duration::from_millis(10)),
            Err(_) => break,
        }
    }
    let _ = child.kill();
    let _ = child.wait();
    None
}

fn supported_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix("aic ") else { return false; };
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()
        || !part.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }
    let numbers: Option<Vec<u64>> = parts.iter().map(|part| part.parse().ok()).collect();
    numbers.is_some_and(|parts| (parts[0], parts[1], parts[2]) >= (0, 3, 2))
}

fn aic_binary_path() -> String {
    #[cfg(test)]
    if let Ok(path) = std::env::var("AIC_TEST_BINARY") {
        if !path.is_empty() {
            return path;
        }
    }
    "aic".to_string()
}

fn verdict_for_exit_code(code: i32) -> &'static str {
    match code {
        0 => "pass",
        1 => "fail",
        2 => "advisory",
        3 => "inconclusive",
        _ => "error",
    }
}

fn report_path_from_stdout(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .last()
        .and_then(|line| line.strip_prefix("report: "))
        .map(str::trim)
        .filter(|path| !path.is_empty())
        .map(str::to_owned)
}

fn read_pipe<R>(mut reader: R) -> std::thread::JoinHandle<std::io::Result<String>>
where
    R: Read + Send + 'static,
{
    std::thread::spawn(move || {
        let mut output = String::new();
        reader.read_to_string(&mut output)?;
        Ok(output)
    })
}

fn join_reader(handle: Option<std::thread::JoinHandle<std::io::Result<String>>>) -> String {
    handle
        .and_then(|handle| handle.join().ok())
        .and_then(Result::ok)
        .unwrap_or_default()
}

fn append_message(base: String, message: &str) -> String {
    if base.trim().is_empty() {
        message.to_string()
    } else if message.is_empty() {
        base
    } else {
        format!("{base}\n{message}")
    }
}

#[cfg(test)]
#[path = "aic_tests.rs"]
mod tests;

fn collect_audit_result(mut child: std::process::Child, timeout_secs: u64) -> AuditResult {
    let stdout_handle = child.stdout.take().map(read_pipe);
    let stderr_handle = child.stderr.take().map(read_pipe);
    let deadline = Instant::now() + Duration::from_secs(timeout_secs);
    let mut status = None;

    while Instant::now() < deadline {
        match child.try_wait() {
            Ok(Some(exit_status)) => {
                status = Some(exit_status);
                break;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(50)),
            Err(err) => {
                let _ = child.kill();
                let _ = child.wait();
                return AuditResult {
                    verdict: "error".to_string(),
                    report_path: None,
                    stdout: join_reader(stdout_handle),
                    stderr: append_message(join_reader(stderr_handle), &err.to_string()),
                    exit_code: None,
                };
            }
        }
    }

    if status.is_none() {
        let _ = child.kill();
        let _ = child.wait();
    }

    audit_result(status, stdout_handle, stderr_handle, timeout_secs)
}

fn audit_result(
    status: Option<std::process::ExitStatus>,
    stdout_handle: Option<std::thread::JoinHandle<std::io::Result<String>>>,
    stderr_handle: Option<std::thread::JoinHandle<std::io::Result<String>>>,
    timeout_secs: u64,
) -> AuditResult {
    let stdout = join_reader(stdout_handle);
    let stderr = join_reader(stderr_handle);
    match status.and_then(|exit_status| exit_status.code()) {
        Some(code) => AuditResult {
            verdict: verdict_for_exit_code(code).to_string(),
            report_path: report_path_from_stdout(&stdout),
            stdout,
            stderr,
            exit_code: Some(code),
        },
        None => AuditResult {
            verdict: "error".to_string(),
            report_path: None,
            stdout,
            stderr: append_message(stderr, &format!("aic audit timed out after {timeout_secs}s")),
            exit_code: None,
        },
    }
}
