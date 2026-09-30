// CLI batch tests using a fake curl and the existing debug-only fake-key switch.
// Covers preflight, failures, JSON/text screening, streaming schema, and resume.

use serde_json::{Value, json};
use std::os::unix::fs::PermissionsExt;
use std::process::Output;
use tempfile::TempDir;

mod common;

struct Fixture(TempDir);

impl Fixture {
    fn new(items: &str) -> Self {
        let dir = tempfile::tempdir().expect("tempdir");
        let curl = dir.path().join("curl");
        std::fs::write(
            &curl,
            concat!(
                "#!/bin/sh\n",
                "dir=\"$(dirname \"$0\")\"\n",
                "printf '%s\\n' \"$@\" > \"$dir/argv.$$\"\n",
                "cat > \"$dir/body.$$\"\necho call >> \"$dir/calls\"\n",
                "cat \"$dir/response\"\nprintf '\\n200'\n"
            ),
        )
        .expect("curl");
        std::fs::set_permissions(&curl, std::fs::Permissions::from_mode(0o755)).expect("executable");
        std::fs::write(dir.path().join("in"), items).expect("input");
        std::fs::write(
            dir.path().join("response"),
            json!({
                "model": "jev-1.13.0", "answers": {"v": {"type": "choice", "choice": "yes"}},
                "usage": {"input_tokens": 7}
            })
            .to_string(),
        )
        .expect("response");
        Self(dir)
    }

    fn run(&self, key: &str, extra: &[&str]) -> Output {
        let dir = self.0.path();
        common::aid_cmd_in(dir)
            .arg("classify")
            .arg("--batch")
            .arg(dir.join("in"))
            .arg("--out")
            .arg(dir.join("out"))
            .args(["--choice", "v=Verdict?", "--options", "yes,no"])
            .args(extra)
            .env("AID_TYPESAFE_TEST_KEY", key)
            .env(
                "PATH",
                format!("{}:{}", dir.display(), std::env::var("PATH").unwrap_or_default()),
            )
            .output()
            .expect("run")
    }

    fn rows(&self) -> Vec<Value> {
        std::fs::read_to_string(self.0.path().join("out"))
            .expect("output")
            .lines()
            .map(|line| serde_json::from_str(line).expect("row"))
            .collect()
    }

    fn calls(&self) -> usize {
        std::fs::read_to_string(self.0.path().join("calls"))
            .map(|text| text.lines().count())
            .unwrap_or(0)
    }
}

#[test]
fn batch_reuses_transport_screens_each_item_and_resumes_successes() {
    let input = [
        json!({"id": "text", "state": "hello"}),
        json!({"id": "json", "state": {"report": "hello"}}),
        json!({"id": "refused", "state": {"key": format!("sk-proj-{}", "A1b2C3d4E5f6G7h8")}}),
    ]
    .map(|value| value.to_string())
    .join("\n");
    let fixture = Fixture::new(&input);
    let output = fixture.run("fake", &[]);
    assert_eq!(
        output.status.code(),
        Some(6),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(stderr.contains("total=3 ok=2 failed=1 skipped=0"), "{stderr}");
    assert!(stderr.contains("\"yes\":2") && stderr.contains("\"no\":0"), "{stderr}");
    assert_eq!(fixture.calls(), 2);
    let rows = fixture.rows();
    for row in rows.iter().filter(|row| row["ok"] == true) {
        assert_eq!(row["model"], "jev-1.13.0");
        assert_eq!(row["usage"]["input_tokens"], 7);
        assert!(row["latency_ms"].is_u64());
    }
    assert_eq!(
        rows.iter().find(|row| row["id"] == "refused").expect("refusal")["error_kind"],
        "refused"
    );
    assert_safe_argv(&fixture);
    let resumed = fixture.run("fake", &["--resume"]);
    assert_eq!(resumed.status.code(), Some(6));
    assert!(String::from_utf8_lossy(&resumed.stderr).contains("total=3 ok=0 failed=1 skipped=2"));
    assert_eq!(fixture.calls(), 2);
    assert_eq!(fixture.rows().len(), 4);
    assert!(!String::from_utf8_lossy(&output.stderr).contains("ts_fake_key_for_tests"));
    assert!(!rows.iter().any(|row| row.to_string().contains("A1b2C3")));
}

fn assert_safe_argv(fixture: &Fixture) {
    for entry in std::fs::read_dir(fixture.0.path()).expect("files").map(Result::unwrap) {
        if entry.file_name().to_string_lossy().starts_with("argv.") {
            assert!(
                !std::fs::read_to_string(entry.path())
                    .expect("argv")
                    .contains("ts_fake_key_for_tests")
            );
        }
    }
}

#[test]
fn duplicate_ids_invalid_questions_and_missing_key_fail_before_http() {
    let fixture = Fixture::new("{\"id\":\"a\",\"state\":\"hello\"}\n{\"id\":\"a\",\"state\":\"hello\"}");
    assert_eq!(fixture.run("fake", &[]).status.code(), Some(4));
    assert!(!fixture.0.path().join("out").exists());
    std::fs::write(fixture.0.path().join("in"), "{\"id\":\"a\",\"state\":\"hello\"}").expect("input");
    assert_eq!(
        fixture
            .run("fake", &["--noul", "bad=Again?", "--noul", "bad=Again?"])
            .status
            .code(),
        Some(4)
    );
    assert_eq!(fixture.run("absent", &[]).status.code(), Some(2));
    assert_eq!(fixture.calls(), 0);
}

#[test]
fn successful_and_empty_resumed_batches_exit_zero_without_reading_stdin() {
    let fixture = Fixture::new("{\"id\":\"a\",\"state\":\"hello\"}");
    let first = fixture.run("fake", &[]);
    assert_eq!(
        first.status.code(),
        Some(0),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert_eq!(fixture.calls(), 1);
    let resumed = fixture.run("absent", &["--resume"]);
    assert_eq!(resumed.status.code(), Some(0));
    assert!(String::from_utf8_lossy(&resumed.stderr).contains("total=1 ok=0 failed=0 skipped=1"));
    assert_eq!(fixture.rows().len(), 1);
    assert_eq!(fixture.calls(), 1);
    assert_eq!(
        fixture.run("absent", &["--resume", "--model", "invalid"]).status.code(),
        Some(4)
    );
}

#[test]
fn questions_file_and_malformed_resume_are_validated_before_sending() {
    let fixture = Fixture::new("{\"id\":\"a\",\"state\":\"hello\"}");
    let questions = fixture.0.path().join("questions.json");
    std::fs::write(&questions, "{}").expect("questions");
    assert_eq!(
        fixture
            .run("fake", &["--questions", questions.to_str().expect("path")])
            .status
            .code(),
        Some(0)
    );
    std::fs::write(fixture.0.path().join("out"), "{partial").expect("partial");
    assert_eq!(fixture.run("fake", &["--resume"]).status.code(), Some(4));
    assert_eq!(fixture.calls(), 1);
}

#[test]
fn guide_and_command_index_cover_batch_flags_and_contract() {
    let guide = include_str!("../default-skills/aid-guide/references/classify.md");
    let index = include_str!("../default-skills/aid-guide/references/command-index.md");
    for term in [
        "--batch items.jsonl --out results.jsonl --jobs 4 --resume",
        "1-16",
        "HTTP 429/529",
        "five attempts",
        "ok:true",
        "error_kind",
        "total / ok / failed / skipped",
        "| 6 | batch completed",
        "No batch MCP tool",
    ] {
        assert!(guide.contains(term), "missing {term}");
    }
    for flag in [
        "--batch <items.jsonl>",
        "--out <results.jsonl>",
        "--jobs N",
        "--resume",
        "6 batch item failures",
    ] {
        assert!(index.contains(flag), "missing {flag}");
    }
}
