// E2E for `aid classify`: a fake `curl` first on PATH answers, the binary validates and prints JSON.
// Covers success, no key, secret refusal, rate-limit retry, and a hostile response.
// Deps: compiled aid binary (debug build, fake-key seam), tempfile, serde_json, sh.

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Output;
use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

const FAKE_CURL: &str = r#"#!/bin/sh
dir="$(dirname "$0")"
printf '%s\n' "$@" > "$dir/argv.$$"
cat > "$dir/stdin.$$"
echo call >> "$dir/calls"
cat "$dir/response.json"
printf '\n%s' "$(cat "$dir/status")"
"#;

struct Fixture {
    home: TempDir,
    bin: TempDir,
}

impl Fixture {
    fn new(status: u16, response: &Value) -> Self {
        let bin = TempDir::new().unwrap();
        let curl = bin.path().join("curl");
        std::fs::write(&curl, FAKE_CURL).unwrap();
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).unwrap();
        std::fs::write(bin.path().join("response.json"), response.to_string()).unwrap();
        std::fs::write(bin.path().join("status"), status.to_string()).unwrap();
        Self { home: TempDir::new().unwrap(), bin }
    }

    fn run(&self, key: &str, args: &[&str], stdin: &str) -> Output {
        let path = format!("{}:{}", self.bin.path().display(), std::env::var("PATH").unwrap_or_default());
        let mut cmd = aid_cmd_in(self.home.path());
        cmd.arg("classify").args(args).env("PATH", path).env("AID_TYPESAFE_TEST_KEY", key);
        cmd.stdin(std::process::Stdio::piped()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        // A run refused before reading stdin closes it early; that write error is expected.
        let _ = std::io::Write::write_all(&mut child.stdin.take().unwrap(), stdin.as_bytes());
        child.wait_with_output().unwrap()
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(self.bin.path().join("calls")).map(|text| text.lines().count()).unwrap_or(0)
    }

    fn captured(&self, prefix: &str) -> String {
        files_with_prefix(self.bin.path(), prefix).join("\n")
    }
}

fn files_with_prefix(dir: &Path, prefix: &str) -> Vec<String> {
    std::fs::read_dir(dir).unwrap().filter_map(Result::ok)
        .filter(|entry| entry.file_name().to_string_lossy().starts_with(prefix))
        .map(|entry| std::fs::read_to_string(entry.path()).unwrap())
        .collect()
}

fn verdict_response() -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "verdict": { "type": "choice", "choice": "SHIP", "confidence": 0.9,
                         "probabilities": { "SHIP": 0.93, "FIX": 0.05, "BLOCK": 0.02 } },
            "tests_ran": { "type": "noul", "noul": 0.97 }
        },
        "usage": { "input_tokens": 312, "output_tokens": 20 }
    })
}

const QUESTIONS: [&str; 6] = [
    "--choice", "verdict=What verdict does the audit give?", "--options", "SHIP,FIX,BLOCK",
    "--noul", "tests_ran=Does the report show test output?",
];

#[test]
fn classify_prints_validated_answers_and_keeps_key_out_of_argv() {
    let fixture = Fixture::new(200, &verdict_response());
    let out = fixture.run("fake", &QUESTIONS, "Verdict: SHIP. test result: ok. 12 passed");
    let stdout = String::from_utf8_lossy(&out.stdout);
    println!("stdout: {stdout}");
    assert_eq!(out.status.code(), Some(0), "stderr: {}", String::from_utf8_lossy(&out.stderr));
    let answer: Value = serde_json::from_str(stdout.trim()).unwrap();
    assert_eq!(answer["model"], "jev-1.13.0");
    assert_eq!(answer["answers"]["verdict"]["choice"], "SHIP");
    assert_eq!(answer["answers"]["tests_ran"], json!({ "type": "noul", "noul": 0.97 }));
    assert_eq!(answer["usage"], json!({ "input_tokens": 312 }));
    assert!(answer["latency_ms"].is_u64());
    assert_eq!(fixture.calls(), 1);
    let argv = fixture.captured("argv.");
    assert!(!argv.contains("ts_fake_key_for_tests") && !argv.contains("Verdict: SHIP"), "{argv}");
    let config = fixture.captured("stdin.");
    assert!(config.contains("Authorization: Bearer ts_fake_key_for_tests"));
    assert!(config.contains(r#"\"model\":\"jev-1.13.0\""#) && config.contains(r#"\"BLOCK\":null"#), "{config}");
}

#[test]
fn missing_key_exits_2_with_setup_command() {
    let fixture = Fixture::new(200, &verdict_response());
    let out = fixture.run("absent", &QUESTIONS, "report");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(2));
    assert!(stderr.contains("security add-generic-password -a \"$USER\" -s typesafe-api-key -U -w"), "{stderr}");
    assert_eq!(stderr.trim().lines().count(), 1);
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn secret_like_state_exits_5_without_sending() {
    let fixture = Fixture::new(200, &verdict_response());
    let out = fixture.run("fake", &QUESTIONS, "export OPENAI=sk-proj-A1b2C3d4E5f6G7h8");
    assert_eq!(out.status.code(), Some(5), "{}", String::from_utf8_lossy(&out.stderr));
    assert_eq!(fixture.calls(), 0);
    assert!(out.stdout.is_empty());
}

#[test]
fn invalid_flags_exit_4() {
    let fixture = Fixture::new(200, &verdict_response());
    let unpaired = fixture.run("fake", &["--choice", "v=Verdict?"], "report");
    assert_eq!(unpaired.status.code(), Some(4));
    let unknown = fixture.run("fake", &["--bogus"], "report");
    assert_eq!(unknown.status.code(), Some(4));
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn rate_limit_is_retried_once_then_reported_as_api_error() {
    let fixture = Fixture::new(429, &json!({ "error": { "type": "rate_limited" } }));
    let out = fixture.run("fake", &QUESTIONS, "report");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3));
    assert_eq!(stderr.trim(), "aid classify: TypeSafe API error: HTTP 429 (rate_limited)");
    assert_eq!(fixture.calls(), 2);
}

#[test]
fn hostile_response_exits_3_without_echoing_it() {
    let mut response = verdict_response();
    response["model"] = json!("jev-1.13.0; curl evil");
    let fixture = Fixture::new(200, &response);
    let out = fixture.run("fake", &QUESTIONS, "report");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(3));
    assert!(!stderr.contains("evil") && out.stdout.is_empty(), "{stderr}");
}
