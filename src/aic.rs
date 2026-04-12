// AIC cross-audit integration helpers.
// Exports: availability detection, timeout parsing, and `aic audit` execution.
// Deps: std::{env, io, process, sync::OnceLock, thread, time}.

use std::io::Read;
use std::path::Path;
use std::process::{Command, Stdio};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

static AIC_AVAILABLE: OnceLock<bool> = OnceLock::new();
const DEFAULT_AUDIT_TIMEOUT_SECS: u64 = 300;
const MAX_AUDIT_TIMEOUT_SECS: u64 = 1_800;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AuditResult {
    pub verdict: String,
    pub report_path: Option<String>,
    pub stdout: String,
    pub stderr: String,
    pub exit_code: Option<i32>,
}

pub fn is_available() -> bool {
    #[cfg(test)]
    {
        return detect_available();
    }
    #[cfg(not(test))]
    *AIC_AVAILABLE.get_or_init(detect_available)
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
    let mut command = Command::new("aic");
    command
        .args(["audit", task_id])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(current_dir) = current_dir {
        command.current_dir(current_dir);
    }

    let mut child = match command.spawn() {
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

fn detect_available() -> bool {
    #[cfg(test)]
    if let Ok(value) = std::env::var("AIC_TEST_PRESENT") {
        return matches!(
            value.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes"
        );
    }

    Command::new("aic")
        .arg("--version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn verdict_for_exit_code(code: i32) -> &'static str {
    if code == 0 {
        "pass"
    } else if (1..=99).contains(&code) {
        "fail"
    } else {
        "error"
    }
}

fn report_path_from_stdout(stdout: &str) -> Option<String> {
    stdout
        .lines()
        .rev()
        .find_map(|line| line.trim().strip_prefix("report: ").map(str::trim))
        .filter(|path| !path.is_empty())
        .map(ToString::to_string)
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

fn join_reader(
    handle: Option<std::thread::JoinHandle<std::io::Result<String>>>,
) -> String {
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
mod tests {
    use super::*;
    use std::env;
    use std::fs;
    use std::os::unix::fs::PermissionsExt;
    use std::sync::{Mutex, OnceLock};

    fn set_env(key: &str, value: impl AsRef<std::ffi::OsStr>) {
        unsafe { env::set_var(key, value) }
    }

    fn remove_env(key: &str) {
        unsafe { env::remove_var(key) }
    }

    fn env_lock() -> std::sync::MutexGuard<'static, ()> {
        static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
        LOCK.get_or_init(|| Mutex::new(())).lock().unwrap()
    }

    fn install_aic_shim(dir: &Path, body: &str) {
        let path = dir.join("aic");
        fs::write(&path, format!("#!/bin/sh\n{body}\n")).unwrap();
        let mut perms = fs::metadata(&path).unwrap().permissions();
        perms.set_mode(0o755);
        fs::set_permissions(&path, perms).unwrap();
    }

    #[test]
    fn is_available_caches_result() {
        let _guard = env_lock();
        set_env("AIC_TEST_PRESENT", "1");
        assert_eq!(is_available(), is_available());
        remove_env("AIC_TEST_PRESENT");
    }

    #[test]
    fn run_audit_parses_pass_report_path() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let original_path = env::var("PATH").unwrap_or_default();
        install_aic_shim(
            temp.path(),
            "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nprintf 'ok\\nreport: /tmp/report.md\\n'\nexit 0",
        );
        set_env("PATH", format!("{}:{}", temp.path().display(), original_path));

        let result = run_audit("t-audit", Some(temp.path()));

        assert_eq!(result.verdict, "pass");
        assert_eq!(result.report_path.as_deref(), Some("/tmp/report.md"));
        set_env("PATH", original_path);
    }

    #[test]
    fn run_audit_times_out() {
        let _guard = env_lock();
        let temp = tempfile::tempdir().unwrap();
        let original_path = env::var("PATH").unwrap_or_default();
        install_aic_shim(
            temp.path(),
            "if [ \"$1\" = \"--version\" ]; then exit 0; fi\nsleep 2\nprintf 'report: /tmp/late.md\\n'\nexit 0",
        );
        set_env("PATH", format!("{}:{}", temp.path().display(), original_path));
        set_env("AID_AUDIT_TIMEOUT_SECS", "1");

        let result = run_audit("t-audit", Some(temp.path()));

        assert_eq!(result.verdict, "error");
        assert_eq!(result.report_path, None);
        remove_env("AID_AUDIT_TIMEOUT_SECS");
        set_env("PATH", original_path);
    }
}
