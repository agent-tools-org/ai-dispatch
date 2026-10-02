// Isolated CLI harness for actual cascade argv, saved profiles and parent links.
// Provides controlled Oz failures, Codex completions and advice comparisons.
// Deps: compiled aid, SQLite, tempfile, Unix shell executables.
use crate::common::aid_cmd_in;
use serde_json::Value;
use std::{
    os::unix::fs::PermissionsExt,
    process::{Command, Output},
};
use tempfile::TempDir;

pub const PROMPT: &str = "Compare documentation";
pub const PROFILE: &[&str] = &[
    "--difficulty",
    "complex",
    "--budget",
    "premium",
    "--urgency",
    "normal",
    "--rigor",
    "standard",
];
pub struct Harness {
    pub home: TempDir,
    codex: TempDir,
    bin: TempDir,
}
pub struct Row {
    pub id: String,
    pub agent: String,
    pub model: Option<String>,
    pub parent: Option<String>,
    pub saved: Value,
}

impl Harness {
    pub fn new(model: Option<&str>, sticky: bool) -> Self {
        let h = Self {
            home: TempDir::new().expect("home"),
            codex: TempDir::new().expect("CLI home"),
            bin: TempDir::new().expect("bin"),
        };
        let mut config = String::new();
        for agent in [
            "gemini",
            "agy",
            "qwen",
            "copilot",
            "opencode",
            "commandcode",
            "cursor",
            "kilo",
            "mimocode",
            "droid",
            "grok",
        ] {
            config.push_str(&format!("[{agent}]\ndisabled = true\n"));
        }
        if let Some(model) = model {
            std::fs::write(
                h.codex.path().join("config.toml"),
                format!("model = '{model}'\n"),
            )
            .expect("CLI config");
            if sticky {
                config.push_str(&format!("[codex]\nmodel = '{model}'\n"));
            }
            h.served(model);
        }
        std::fs::write(h.home.path().join("agent_config.toml"), config).expect("agents");
        std::fs::create_dir(h.home.path().join("teams")).expect("teams");
        std::fs::write(h.home.path().join("teams/routes.toml"),
            "[team]\nid = 'routes'\ndisplay_name = 'Routes'\npreferred_agents = ['claude']\ndefault_agent = 'claude'\n").expect("team");
        for (name, content) in [("codex", CODEX), ("oz", OZ), ("claude", CLAUDE)] {
            let script = h.bin.path().join(name);
            std::fs::write(&script, content).expect("script");
            std::fs::set_permissions(script, std::fs::Permissions::from_mode(0o755))
                .expect("executable");
        }
        h.mode("quota");
        h
    }

    pub fn aid(&self) -> Command {
        let mut cmd = aid_cmd_in(self.home.path());
        cmd.env("HOME", self.home.path())
            .env("CODEX_HOME", self.codex.path())
            .env("CASCADE_CAPTURE_ROOT", self.home.path())
            .env(
                "PATH",
                format!(
                    "{}:{}",
                    self.bin.path().display(),
                    std::env::var("PATH").expect("PATH")
                ),
            );
        for key in [
            "AID_CALLER_KIND",
            "AID_CALLER_SESSION",
            "AID_CALLER_MODEL",
            "CODEX_THREAD_ID",
            "CLAUDECODE_SESSION_ID",
            "CLAUDE_CODE_SESSION_ID",
        ] {
            cmd.env_remove(key);
        }
        cmd
    }

    pub fn run(&self) -> Command {
        let mut cmd = self.aid();
        cmd.args([
            "run",
            "oz",
            PROMPT,
            "--model",
            "parent-model",
            "--id",
            "t-primary",
            "--kind",
            "refactoring",
            "--team",
            "routes",
            "--no-hint",
            "--no-skill",
            "--no-audit",
        ])
        .args(PROFILE)
        .arg("--dir")
        .arg(self.home.path())
        .arg("--output")
        .arg(self.home.path().join("answer.md"));
        cmd
    }

    pub fn advice(&self) -> Value {
        let output = self
            .aid()
            .args([
                "advise",
                PROMPT,
                "--kind",
                "refactoring",
                "--team",
                "routes",
                "--json",
                "--top",
                "0",
            ])
            .args(PROFILE)
            .output()
            .expect("advice");
        success(&output);
        serde_json::from_slice(&output.stdout).expect("report")
    }

    pub fn first_peer(&self) -> Option<Value> {
        self.advice()["candidates"]
            .as_array()
            .expect("candidates")
            .iter()
            .find(|c| {
                c["eligible"] == true
                    && c["quota"]["status"] != "held"
                    && c["agent"] != "oz"
                    && c["agent"] != "claude"
            })
            .cloned()
    }

    pub fn served(&self, model: &str) {
        std::fs::write(
            self.codex.path().join("models_cache.json"),
            format!(r#"{{"models":[{{"slug":"{model}"}}]}}"#),
        )
        .expect("served models");
    }

    pub fn hold(&self, agent: &str) {
        std::fs::write(
            self.home.path().join(format!("rate-limit-{agent}")),
            "hold: manual\nmessage: quota exhausted\n",
        )
        .expect("hold");
    }

    pub fn mode(&self, mode: &str) {
        std::fs::write(self.home.path().join("mode"), mode).expect("mode");
    }

    pub fn rows(&self) -> Vec<Row> {
        let conn = rusqlite::Connection::open(self.home.path().join("aid.db")).expect("store");
        let mut stmt = conn.prepare("SELECT id, agent, model, parent_task_id, dispatch_args FROM tasks ORDER BY created_at, id").expect("rows");
        stmt.query_map([], |r| {
            Ok((
                r.get::<_, String>(0)?,
                r.get::<_, String>(1)?,
                r.get::<_, Option<String>>(2)?,
                r.get::<_, Option<String>>(3)?,
                r.get::<_, String>(4)?,
            ))
        })
        .expect("query")
        .map(|row| {
            let (id, agent, model, parent, saved) = row.expect("task");
            Row {
                id,
                agent,
                model,
                parent,
                saved: serde_json::from_str(&saved).expect("saved args"),
            }
        })
        .collect()
    }

    pub fn assert_route(&self, row: &Row, candidate: &Value) {
        assert_eq!(row.agent, candidate["agent"].as_str().expect("agent"));
        assert_eq!(row.model.as_deref(), candidate["model"].as_str());
        let argv =
            std::fs::read_to_string(self.home.path().join("codex.args")).expect("actual argv");
        let args: Vec<_> = argv.lines().collect();
        assert_eq!(
            args.windows(2).find(|a| a[0] == "-m").map(|a| a[1]),
            candidate["model"].as_str(),
            "{argv}"
        );
        assert!(
            !args.contains(&"parent-model") && !args.contains(&"resume"),
            "{argv}"
        );
        for (key, value) in [
            ("declared_difficulty", "complex"),
            ("declared_budget", "premium"),
            ("declared_urgency", "normal"),
            ("declared_rigor", "standard"),
            ("kind", "refactoring"),
            ("team", "routes"),
            ("model_source", "Advised"),
        ] {
            assert_eq!(row.saved[key], value, "{key}");
        }
        assert_eq!(
            row.saved["force_default_model"],
            candidate["model"].is_null()
        );
        assert!(!self.home.path().join("claude.args").exists());
    }
}

pub fn success(output: &Output) {
    assert!(
        output.status.success(),
        "stdout={} stderr={}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

const CODEX: &str = r#"#!/bin/sh
if [ "$1" = '--version' ]; then echo 'codex-cli 0.147.0'; exit 0; fi
if [ "$2" = '--help' ]; then echo '      --approve-for-me'; exit 0; fi
printf '%s\n' "$@" > "$CASCADE_CAPTURE_ROOT/codex.args"
printf '%s\n' "$CASCADE_SYNTHETIC_INLINE" "$CASCADE_SYNTHETIC_FORWARDED" "$AID_SHARED_DIR" > "$CASCADE_CAPTURE_ROOT/codex.env"
previous=''
for arg in "$@"; do
  if [ "$previous" = '-o' ]; then printf 'Validated route and profile.\n' > "$arg"; fi
  previous="$arg"
done
printf '%s\n' '{"type":"item.completed","item":{"id":"final","type":"agent_message","text":"Validated the route and profile. Preserved the selected model. Completed all checks."}}'
printf '%s\n' '{"type":"turn.completed","usage":{"input_tokens":20,"cached_input_tokens":10,"output_tokens":80}}'
"#;
const OZ: &str = r#"#!/bin/sh
if [ "$1" = '--version' ]; then echo 'Warp oz 1.0.0'; exit 0; fi
if [ "$1" = '--help' ]; then echo 'Warp Oz agent'; exit 0; fi
printf '%s\n' "$@" > "$CASCADE_CAPTURE_ROOT/oz.args"
if [ "$(cat "$CASCADE_CAPTURE_ROOT/mode")" = 'quota' ]; then
  echo 'Error: Quota limit reached.' >&2
else
  echo 'Synthetic dispatch failure.' >&2
fi
exit 1
"#;
const CLAUDE: &str = r#"#!/bin/sh
if [ "$1" = '--version' ]; then echo 'Claude Code 1.0.0'; exit 0; fi
if [ "$1" = '--help' ]; then echo 'Claude Code'; exit 0; fi
printf '%s\n' "$@" > "$CASCADE_CAPTURE_ROOT/claude.args"
exit 7
"#;
