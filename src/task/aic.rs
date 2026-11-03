// AIC cross-audit integration helpers.
// Exports: availability detection, timeout parsing, and `aic audit` execution.
// Deps: std::{env, io, process, sync::OnceLock, thread, time}.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

#[cfg(not(test))]
static AIC_STATUS: OnceLock<AicStatus> = OnceLock::new();
const DEFAULT_AUDIT_TIMEOUT_SECS: u64 = 300;
const MAX_AUDIT_TIMEOUT_SECS: u64 = 1_800;

#[cfg(test)]
pub(crate) fn test_env_lock() -> std::sync::MutexGuard<'static, ()> {
    use std::sync::Mutex;
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
            Self::TooOld(version) => format!("TooOld({version}): requires aic 0.3.1 or newer"),
            Self::Ready => "Ready".to_string(),
        };
        format!("{description}; install: curl -fsSL https://aid.agent-tools.org/install.sh | sh")
    }
}

pub fn status() -> AicStatus {
    #[cfg(test)]
    {
        detect_status()
    }
    #[cfg(not(test))]
    {
        AIC_STATUS.get_or_init(detect_status).clone()
    }
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
    let Ok(version) = Command::new(aic_binary_path()).arg("--version").output() else {
        return AicStatus::Missing;
    };
    if !version.status.success() {
        return AicStatus::Missing;
    }
    let help = Command::new(aic_binary_path()).arg("--help").output();
    if !help.is_ok_and(|output| String::from_utf8_lossy(&output.stdout)
        .lines().any(|line| line == "Cross-audit aid agent outputs"))
    {
        return AicStatus::Foreign;
    }
    let version = String::from_utf8_lossy(&version.stdout).trim().to_string();
    if !supported_version(&version) {
        return AicStatus::TooOld(version);
    }
    AicStatus::Ready
}

fn supported_version(version: &str) -> bool {
    let Some(version) = version.strip_prefix("aic ") else { return false; };
    let parts: Vec<_> = version.split('.').collect();
    if parts.len() != 3 || parts.iter().any(|part| part.is_empty()
        || !part.bytes().all(|byte| byte.is_ascii_digit())) {
        return false;
    }
    let numbers: Option<Vec<u64>> = parts.iter().map(|part| part.parse().ok()).collect();
    numbers.is_some_and(|parts| (parts[0], parts[1], parts[2]) >= (0, 3, 1))
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
