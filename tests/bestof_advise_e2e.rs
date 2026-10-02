// Controlled CLI races prove that best-of launches the advised model, not the parent model.
// Checks actual argv, saved provenance/profile, cycling, task IDs and winner artifacts.
// Deps: compiled aid, isolated homes, fake Codex, SQLite and tempfile.
#![cfg(unix)]

mod common;
use common::aid_cmd_in;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Output};
use tempfile::TempDir;

const PROFILE: &[&str] = &[
    "--difficulty", "moderate", "--budget", "standard", "--urgency", "normal", "--rigor", "standard",
];

struct Harness { home: TempDir, codex: TempDir, bin: TempDir }

impl Harness {
    fn new(model: Option<&str>, sticky: bool) -> Self {
        let h = Self {
            home: TempDir::new().expect("aid home"), codex: TempDir::new().expect("codex home"),
            bin: TempDir::new().expect("fake binaries"),
        };
        let mut config = String::new();
        for agent in ["gemini", "agy", "qwen", "copilot", "opencode", "commandcode", "cursor", "kilo", "mimocode", "droid", "oz", "claude", "grok"] {
            config.push_str(&format!("[{agent}]\ndisabled = true\n"));
        }
        if let Some(model) = model {
            std::fs::write(h.codex.path().join("config.toml"), format!("model = '{model}'\n")).expect("CLI config");
            if sticky { config.push_str(&format!("[codex]\nmodel = '{model}'\n")); }
            std::fs::write(h.codex.path().join("models_cache.json"), format!(r#"{{"models":[{{"slug":"{model}"}},{{"slug":"parent-model"}}]}}"#)).expect("served models");
        }
        std::fs::write(h.home.path().join("agent_config.toml"), config).expect("agent config");
        let script = h.bin.path().join("codex");
        std::fs::write(&script, FAKE_CODEX).expect("fake codex");
        std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755)).expect("executable");
        h
    }

    fn aid(&self) -> Command {
        let mut cmd = aid_cmd_in(self.home.path());
        cmd.env("HOME", self.home.path()).env("CODEX_HOME", self.codex.path())
            .env("PATH", format!("{}:{}", self.bin.path().display(), std::env::var("PATH").expect("PATH")));
        for key in ["AID_CALLER_KIND", "AID_CALLER_SESSION", "AID_CALLER_MODEL", "CODEX_THREAD_ID", "CLAUDECODE_SESSION_ID", "CLAUDE_CODE_SESSION_ID"] {
            cmd.env_remove(key);
        }
        cmd
    }

    fn race(&self, profile: bool) -> Output {
        let mut cmd = self.aid();
        cmd.args(["run", "codex", "Refactor validation", "--kind", "refactoring", "--best-of", "2",
            "--model", "parent-model", "--id", "t-model-race", "--no-hint", "--no-skill"])
            .arg("--output").arg(self.home.path().join("answer.md"));
        if profile { cmd.args(PROFILE); }
        cmd.output().expect("run best-of")
    }
}

fn assert_success(output: &Output) {
    assert!(output.status.success(), "stdout={} stderr={}", String::from_utf8_lossy(&output.stdout), String::from_utf8_lossy(&output.stderr));
}

fn assert_launches(h: &Harness, model: Option<&str>) {
    let conn = rusqlite::Connection::open(h.home.path().join("aid.db")).expect("store");
    let mut stmt = conn.prepare("SELECT id, model, dispatch_args FROM tasks ORDER BY id").expect("rows");
    let rows: Vec<(String, Option<String>, String)> = stmt.query_map([], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .expect("query").map(|row| row.expect("task")).collect();
    assert_eq!(rows.len(), 2, "cycling must launch two racers");
    for (idx, (id, requested, saved)) in rows.iter().enumerate() {
        assert_eq!(id, &format!("t-model-race-bo{}", idx + 1));
        assert_eq!(requested.as_deref(), model);
        let saved: serde_json::Value = serde_json::from_str(saved).expect("saved dispatch");
        assert_eq!(saved["model_source"], "Advised");
        for (field, value) in [("declared_difficulty", "moderate"), ("declared_budget", "standard"), ("declared_urgency", "normal"), ("declared_rigor", "standard")] {
            assert_eq!(saved[field], value);
        }
        let output = if idx == 0 { "answer.md" } else { "answer-bo2.md" };
        let args = std::fs::read_to_string(h.home.path().join(format!("{output}.args"))).expect("actual launch argv");
        assert!(!args.lines().any(|arg| arg == "parent-model"), "{args}");
        let actual: Vec<_> = args.lines().collect();
        let pinned = actual.windows(2).find(|pair| pair[0] == "-m").map(|pair| pair[1]);
        assert_eq!(pinned, model, "actual CLI launch: {args}");
    }
    assert_eq!(std::fs::read_to_string(h.home.path().join("answer.md")).expect("winner"),
        std::fs::read_to_string(h.home.path().join("answer-bo2.md")).expect("candidate winner"));
}

#[test]
fn bestof_launches_advised_sticky_and_cli_default_models_over_same_agent_explicit_model() {
    for sticky in [true, false] {
        let h = Harness::new(Some("gpt-6-sol"), sticky);
        let advice = h.aid().args(["advise", "Refactor validation", "--kind", "refactoring", "--json", "--top", "0"])
            .args(PROFILE).output().expect("advice");
        assert_success(&advice);
        let report: serde_json::Value = serde_json::from_slice(&advice.stdout).expect("report");
        assert_eq!(report["candidates"][0]["model"], "gpt-6-sol");
        assert_eq!(report["candidates"][0]["pinned"], sticky);
        let output = h.race(true);
        assert_success(&output);
        assert!(String::from_utf8_lossy(&output.stdout).contains("picked t-model-race-bo2"));
        assert_launches(&h, Some("gpt-6-sol"));
    }
}

#[test]
fn bestof_unknown_default_drops_parent_model_and_materializes_missing_profile() {
    let h = Harness::new(None, false);
    let output = h.race(false);
    assert_success(&output);
    assert_launches(&h, None);
}

#[test]
fn bestof_with_no_launchable_candidates_fails_without_task_rows_or_launches() {
    let h = Harness::new(Some("gpt-6-sol"), true);
    std::fs::write(h.home.path().join("rate-limit-codex"), "hold: manual\nmessage: quota exhausted\n").expect("hold");
    let output = h.race(true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no launchable advise candidates"));
    let conn = rusqlite::Connection::open(h.home.path().join("aid.db")).expect("store");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0)).expect("count"), 0);
    assert!(!h.home.path().join("answer.md.args").exists());
}

#[test]
fn bestof_unserved_selected_model_fails_instead_of_launching_another_default() {
    let h = Harness::new(Some("gpt-6-sol"), true);
    std::fs::write(h.codex.path().join("models_cache.json"), r#"{"models":[{"slug":"parent-model"}]}"#).expect("served models");
    let output = h.race(true);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("refusing a different default"));
    assert!(!h.home.path().join("answer.md.args").exists());
}

#[test]
fn bestof_caller_pool_filter_excludes_a_weaker_model_before_launch() {
    let h = Harness::new(Some("gpt-5.6-luna"), true);
    let output = h.aid().env("AID_CALLER_KIND", "codex").env("AID_CALLER_SESSION", "synthetic-caller")
        .env("AID_CALLER_MODEL", "gpt-5.6-sol")
        .args(["run", "codex", "Refactor validation", "--kind", "refactoring", "--best-of", "2", "--no-hint", "--no-skill"])
        .args(PROFILE).output().expect("race");
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("no launchable advise candidates"));
    let conn = rusqlite::Connection::open(h.home.path().join("aid.db")).expect("store");
    assert_eq!(conn.query_row("SELECT COUNT(*) FROM tasks", [], |row| row.get::<_, i64>(0)).expect("count"), 0);
}

const FAKE_CODEX: &str = r#"#!/bin/sh
if [ "$1" = "--version" ]; then echo 'codex-cli 0.147.0'; exit 0; fi
if [ "$2" = "--help" ]; then echo '      --approve-for-me'; exit 0; fi
out=''
previous=''
for arg in "$@"; do
  if [ "$previous" = '-o' ]; then out="$arg"; fi
  previous="$arg"
done
[ -n "$out" ] || exit 2
printf '%s\n' "$@" > "$out.args"
printf 'Validated output for %s\n' "$out" > "$out"
case "$out" in
  *-bo2.md) printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"Validated the scheduler.\nChecked every route.\nKept the selected model.\nPreserved output.\nCompleted the review."}}' ;;
  *) printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"Validated the scheduler."}}' ;;
esac
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":80}}'
"#;
