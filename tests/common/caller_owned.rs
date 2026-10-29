// Black-box caller-owned harness with isolated homes and a counted shell agent.
// Commands are bounded; observations use only public CLI JSON, never the store.
// Deps: common command helpers, serde_json, tempfile, libc, POSIX shell and git.

use serde_json::Value;
use std::cell::RefCell;
use std::fs;
use std::process::{Command, Stdio};
#[path = "caller_owned_process.rs"]
mod process;
#[allow(unused_imports)]
pub use process::{bounded, finish, spawn};
use std::time::{Duration, Instant};
use tempfile::TempDir;

pub const MODEL: &str = "caller-model";
pub const AGENT: &str = "controlled";
pub const PROTOCOL: &str = "aid.execution/1";

#[allow(dead_code)]
pub struct Harness {
    root: TempDir,
    ids: RefCell<Vec<String>>,
}

#[allow(dead_code)]
impl Harness {
    pub fn new(body: &str) -> Self {
        let h = Self {
            root: TempDir::new().unwrap(),
            ids: RefCell::new(Vec::new()),
        };
        for dir in ["home", "aid/agents", "project"] {
            fs::create_dir_all(h.path(dir)).unwrap();
        }
        let script = format!(
            "#!/bin/sh\nprintf 'invoked\\n' >> '{}'\n\
             selected=''\nprevious=''\nfor arg in \"$@\"; do\n\
             if [ \"$previous\" = '--model' ]; then selected=$arg; fi\n\
             previous=$arg\ndone\nprintf '%s\\n' \"$selected\" >> '{}'\n{}\n",
            h.path("count").display(),
            h.path("models").display(),
            body
        );
        crate::common::write_executable(&h.path("controlled-agent"), &script);
        fs::write(
            h.path("aid/agents/controlled.toml"),
            format!(
                "[agent]\nid = \"{AGENT}\"\ndisplay_name = \"controlled\"\ncommand = \"{}\"\n\
             model_flag = \"--model\"\nstreaming = true\noutput_format = \"jsonl\"\n\
             interactive_input = false\ntrust_tier = \"local\"\n",
                h.path("controlled-agent").display()
            ),
        )
        .unwrap();
        crate::common::seed_codex_leaderboard(&h.path("aid"));
        h
    }

    pub fn path(&self, name: &str) -> std::path::PathBuf {
        self.root.path().join(name)
    }

    pub fn aid(&self) -> Command {
        let mut cmd = crate::common::aid_cmd_with_cwd(&self.path("aid"), &self.path("project"));
        cmd.env("HOME", self.path("home"))
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env_remove("AID_NO_DETACH")
            .stdin(Stdio::null());
        cmd
    }

    pub fn run(&self, id: &str, extra: &[&str]) -> Command {
        self.ids.borrow_mut().push(id.to_string());
        let mut cmd = self.aid();
        cmd.args([
            "run",
            AGENT,
            "exercise caller owned execution",
            "--id",
            id,
            "--policy",
            "caller-owned",
            "--model",
            MODEL,
            "--json",
            "--bg",
            "--no-hint",
            "--no-skill",
            "--timeout",
            "15",
        ])
        .arg("--dir")
        .arg(self.path("project"))
        .args(extra);
        cmd
    }

    pub fn launch(&self, id: &str, extra: &[&str]) -> Value {
        let receipt = json(self.run(id, extra), 0);
        assert_eq!(receipt["submission"], "launched");
        identity(&receipt, id);
        receipt
    }

    pub fn observe(&self, id: &str) -> Value {
        let (code, value) = result(self.aid().args(["wait", id, "--json", "--timeout", "0"]));
        assert!(code == 0 || code == 124, "exit {code}: {value}");
        identity(&value, id);
        assert_eq!(code == 0, value["settled"] == true);
        value
    }

    pub fn settled(&self, id: &str) -> Value {
        let value = json(self.aid().args(["wait", id, "--json", "--timeout", "25"]), 0);
        identity(&value, id);
        assert_eq!(value["settled"], true);
        assert_eq!(value["worker"]["state"], "settled");
        assert_eq!(value["task"]["terminal"], true);
        value
    }

    pub fn stop(&self, id: &str) -> Value {
        let value = json(self.aid().args(["stop", id, "--json"]), 0);
        identity(&value, id);
        assert_eq!(value["settled"], true);
        value
    }

    pub fn count(&self) -> usize {
        fs::read_to_string(self.path("count"))
            .unwrap_or_default()
            .lines()
            .count()
    }

    pub fn assert_rows(&self, id: &str, count: usize) {
        assert_eq!(self.count(), count, "agent invocation count");
        let models = fs::read_to_string(self.path("models")).unwrap();
        assert_eq!(models.lines().collect::<Vec<_>>(), vec![MODEL; count]);
        let board = json(self.aid().args(["board", "--json"]), 0);
        let rows = board.as_array().expect("board array");
        assert_eq!(rows.len(), 1, "no child or auto-suffixed rows: {board}");
        assert_eq!(rows[0]["id"], id);
        let show = json(self.aid().args(["show", id, "--json"]), 0);
        identity(&show["execution"], id);
    }

    pub fn assert_empty(&self, id: &str) {
        assert_eq!(self.count(), 0);
        assert_eq!(
            json(self.aid().args(["board", "--json"]), 0),
            serde_json::json!([])
        );
        let output = bounded(self.aid().args(["show", id, "--json"]));
        assert!(!output.status.success(), "refusal created task {id}");
    }

    pub fn wait_for(&self, mut predicate: impl FnMut() -> bool) {
        let deadline = Instant::now() + Duration::from_secs(15);
        while !predicate() {
            assert!(Instant::now() < deadline, "fixture condition timed out");
            std::thread::sleep(Duration::from_millis(25));
        }
    }

    pub fn wait_invoked(&self) {
        self.wait_for(|| self.count() == 1);
    }

    pub fn gate_script(&self) -> String {
        format!(
            "touch '{}'\ni=0\nwhile [ ! -f '{}' ] && [ $i -lt 400 ]; do\n\
             sleep 0.05\ni=$((i+1))\ndone\n[ -f '{}' ] || exit 2\n",
            self.path("holding").display(),
            self.path("release").display(),
            self.path("release").display()
        )
    }

    pub fn verifier(&self) -> String {
        crate::common::write_executable(
            &self.path("verify"),
            &format!("#!/bin/sh\n{}exit 0\n", self.gate_script()),
        );
        self.path("verify").to_str().unwrap().to_string()
    }

    pub fn release(&self) {
        fs::write(self.path("release"), "go").unwrap();
    }

    pub fn terminal_window(&self, id: &str) -> Value {
        let verifier = self.verifier();
        self.launch(id, &["--verify", &verifier]);
        self.wait_for(|| self.path("holding").exists());
        let value = json(self.aid().args(["wait", id, "--json", "--timeout", "0"]), 124);
        identity(&value, id);
        assert_eq!(value["task"]["terminal"], true);
        assert_eq!(value["worker"]["state"], "running");
        assert_eq!(value["settled"], false);
        value
    }

    pub fn git_project(&self) {
        fs::write(self.path("project/tracked.txt"), "initial\n").unwrap();
        for args in [
            vec!["init", "-q", "-b", "main"],
            vec!["add", "tracked.txt"],
            vec![
                "-c",
                "user.name=fixture",
                "-c",
                "user.email=fixture@example.test",
                "commit",
                "-qm",
                "fixture",
            ],
        ] {
            let mut cmd = Command::new("git");
            cmd.current_dir(self.path("project")).args(args);
            assert!(bounded(&mut cmd).status.success());
        }
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        let _ = fs::write(self.path("release"), "go");
        for id in self.ids.borrow().iter() {
            let _ = bounded(self.aid().args(["stop", id, "--json", "--force"]));
        }
    }
}

pub fn identity(value: &Value, id: &str) {
    assert_eq!(value["protocol"], PROTOCOL);
    assert_eq!(value["task_id"], id);
    assert!(value["revision"].as_u64().is_some_and(|r| r >= 1));
    assert_eq!(value["task"]["requested_model"], MODEL);
    for key in [
        "status",
        "terminal",
        "agent",
        "observed_model",
        "attribution_source",
        "exit_code",
        "completed_at",
    ] {
        assert!(
            value["task"].as_object().unwrap().contains_key(key),
            "missing task.{key}"
        );
    }
    for key in ["state", "outcome", "pid", "settled_at", "detail"] {
        assert!(
            value["worker"].as_object().unwrap().contains_key(key),
            "missing worker.{key}"
        );
    }
    assert!(value["observed_at"].as_str().is_some());
    assert_eq!(value["policy"]["name"], "caller-owned");
    assert_eq!(value["policy"]["version"], 1);
    assert_eq!(value["policy"]["agent"], AGENT);
    assert_eq!(value["policy"]["model"], MODEL);
    assert!(value["policy"]["accepted_at"].as_str().is_some());
    assert_eq!(value["settled"] == true, value["worker"]["state"] == "settled");
}

pub fn result(mut cmd: impl std::borrow::BorrowMut<Command>) -> (i32, Value) {
    let out = bounded(std::borrow::BorrowMut::borrow_mut(&mut cmd));
    let code = out.status.code().expect("CLI exited by signal");
    let value = serde_json::from_slice(&out.stdout).unwrap_or_else(|err| {
        panic!(
            "invalid JSON ({err}), exit {code}: stdout={} stderr={}",
            String::from_utf8_lossy(&out.stdout),
            String::from_utf8_lossy(&out.stderr)
        )
    });
    (code, value)
}

pub fn json(cmd: impl std::borrow::BorrowMut<Command>, expected: i32) -> Value {
    let (code, value) = result(cmd);
    assert_eq!(code, expected, "{value}");
    value
}

pub const SUCCESS: &str = "printf '%s\\n' '{\"type\":\"completion\",\"finalText\":\"done\"}'";
