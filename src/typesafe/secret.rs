// TypeSafe API key access and the one authenticated HTTP call that uses it.
// Exports: api_key, post_json. The key is read from the macOS login keychain at call time,
// travels to curl only on stdin, and never enters argv, env, logs, or task state.

use anyhow::{Context, Result, bail};
use std::io::{Read, Write};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

const KEYCHAIN_SERVICE: &str = "typesafe-api-key";
const SECURITY_BIN: &str = "/usr/bin/security";
const KEYCHAIN_DEADLINE: Duration = Duration::from_secs(3);

/// The key from the login keychain, or None when absent, empty, or not on macOS.
/// No env fallback: an exported key would be inherited by every dispatched agent.
pub(crate) fn api_key() -> Option<String> {
    if !cfg!(target_os = "macos") {
        return None;
    }
    let account = std::env::var("USER").ok()?;
    let mut lookup = Command::new(SECURITY_BIN);
    lookup
        .args(["find-generic-password", "-a", &account, "-s", KEYCHAIN_SERVICE, "-w"])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null());
    // A keychain access prompt or a locked keychain must not stall the caller.
    let stdout = output_within(lookup, KEYCHAIN_DEADLINE)?;
    // An item stored from a TTY-less prompt is empty and exits 0; treat it as absent.
    let key = String::from_utf8(stdout).ok()?.trim().to_string();
    (!key.is_empty()).then_some(key)
}

/// Stdout of a successful run that finishes before `deadline`; otherwise the child is killed.
fn output_within(mut command: Command, deadline: Duration) -> Option<Vec<u8>> {
    let mut child = command.spawn().ok()?;
    let started = Instant::now();
    loop {
        match child.try_wait() {
            Ok(Some(status)) if status.success() => break,
            Ok(Some(_)) | Err(_) => return None,
            Ok(None) if started.elapsed() >= deadline => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
            Ok(None) => std::thread::sleep(Duration::from_millis(20)),
        }
    }
    let mut stdout = Vec::new();
    child.stdout.take()?.read_to_end(&mut stdout).ok()?;
    Some(stdout)
}

/// POST `body` (JSON) to `url` with a bearer key. Returns (HTTP status, response body).
pub(crate) fn post_json(url: &str, key: &str, body: &str, timeout_secs: u64) -> Result<(u16, String)> {
    let config = curl_config(key, body)?;
    let mut child = Command::new("curl")
        .args(curl_args(url, timeout_secs))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .context("spawn curl")?;
    child
        .stdin
        .take()
        .context("curl stdin")?
        .write_all(config.as_bytes())
        .context("write curl config")?;
    let output = child.wait_with_output().context("wait for curl")?;
    parse_status_suffix(&String::from_utf8_lossy(&output.stdout))
}

/// argv carries no secret and no request body; both arrive through `-K -`.
/// `-q` must be first: it stops curl reading a `.curlrc` that could redirect or trace the key.
fn curl_args(url: &str, timeout_secs: u64) -> Vec<String> {
    vec![
        "-q".into(), "-sS".into(), "-K".into(), "-".into(), "-X".into(), "POST".into(),
        "-H".into(), "Content-Type: application/json".into(),
        "--max-time".into(), timeout_secs.to_string(),
        "-w".into(), "\n%{http_code}".into(), url.into(),
    ]
}

fn curl_config(key: &str, body: &str) -> Result<String> {
    if key.is_empty() || key.chars().any(|c| matches!(c, '"' | '\\' | '\n' | '\r')) {
        bail!("refusing a key with characters that would break the curl config");
    }
    Ok(format!(
        "header = \"Authorization: Bearer {key}\"\ndata-binary = \"{}\"\n",
        quote_config_value(body)
    ))
}

fn quote_config_value(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"").replace('\n', "\\n").replace('\r', "\\r")
}

fn parse_status_suffix(stdout: &str) -> Result<(u16, String)> {
    let (body, status) = stdout.rsplit_once('\n').context("curl output has no status line")?;
    let code = status.trim().parse::<u16>().context("curl status is not a number")?;
    if code == 0 {
        bail!("request did not complete (network error or timeout)");
    }
    Ok((code, body.to_string()))
}

#[cfg(test)]
#[path = "secret_tests.rs"]
mod tests;
