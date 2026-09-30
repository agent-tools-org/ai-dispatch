// Settlement E2E harness: temp AID_HOME, a shell custom agent, and a verifier held on a gate file.
// Exports Harness with gate release, task status probes, and aid command builders.
// Deps: tests/common aid_cmd_with_cwd, tempfile, rusqlite, and a POSIX shell.

use rusqlite::Connection;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};
use tempfile::TempDir;

use crate::common::aid_cmd_with_cwd;

pub(crate) const AGENT: &str = "settle";
pub(crate) const TERMINAL: [&str; 5] = ["done", "merged", "failed", "skipped", "stopped"];
const RELEASE_WAIT: Duration = Duration::from_secs(60);

pub(crate) struct Harness {
    home: TempDir,
    project: TempDir,
    _scripts: TempDir,
    gate: PathBuf,
    verifier: PathBuf,
}

impl Harness {
    pub(crate) fn new() -> Self {
        let home = TempDir::new().unwrap();
        let project = TempDir::new().unwrap();
        let scripts = TempDir::new().unwrap();
        let gate = scripts.path().join("gate");
        let agent = write_script(scripts.path(), "settle-agent", AGENT_SCRIPT);
        // The verifier announces itself, holds until the test writes an exit code into the gate,
        // then consumes the gate so the next verify (for example a retry) holds again.
        let verifier = write_script(
            scripts.path(),
            "verifier",
            &format!(
                "#!/bin/sh\ng='{}'\ntouch \"$g.waiting\"\ni=0\n\
                 while [ ! -f \"$g\" ] && [ $i -lt 600 ]; do sleep 0.2; i=$((i+1)); done\n\
                 [ -f \"$g\" ] || exit 2\ncode=$(cat \"$g\")\nrm -f \"$g\"\nexit \"$code\"\n",
                gate.display()
            ),
        );
        write_custom_agent(home.path(), &agent);
        Self { home, project, _scripts: scripts, gate, verifier }
    }

    /// Harness whose project is a git repo with one commit, for worktree-backed runs.
    pub(crate) fn with_git_project() -> Self {
        let harness = Self::new();
        let dir = harness.project();
        std::fs::write(dir.join("README.md"), "settlement\n").unwrap();
        for args in [
            &["init", "-q", "-b", "main"][..],
            &["add", "README.md"],
            &["-c", "user.name=t", "-c", "user.email=t@example.com", "commit", "-qm", "init"],
        ] {
            assert!(Command::new("git").args(args).current_dir(dir).status().unwrap().success());
        }
        harness
    }

    pub(crate) fn home(&self) -> &Path {
        self.home.path()
    }

    pub(crate) fn project(&self) -> &Path {
        self.project.path()
    }

    pub(crate) fn verifier(&self) -> &Path {
        &self.verifier
    }

    pub(crate) fn aid(&self) -> Command {
        aid_cmd_with_cwd(self.home(), self.project())
    }

    /// `aid run` for the shell agent with the gated verifier; `extra` appends flags.
    pub(crate) fn run(&self, id: &str, extra: &[&str]) -> Command {
        let mut cmd = self.aid();
        cmd.args(["run", AGENT, "settle the task", "--id", id])
            .arg("--dir")
            .arg(self.project())
            .arg("--verify")
            .arg(&self.verifier)
            .args(extra);
        cmd
    }

    /// Block until a verifier is holding, then release it with `code`.
    pub(crate) fn release(&self, code: i32) {
        self.wait_verifier_holding();
        let _ = std::fs::remove_file(self.marker());
        let tmp = self.gate.with_extension("tmp");
        std::fs::write(&tmp, code.to_string()).unwrap();
        std::fs::rename(&tmp, &self.gate).unwrap();
    }

    pub(crate) fn wait_verifier_holding(&self) {
        let deadline = Instant::now() + RELEASE_WAIT;
        while !self.marker().exists() {
            assert!(Instant::now() < deadline, "verifier never started");
            thread::sleep(Duration::from_millis(50));
        }
    }

    fn marker(&self) -> PathBuf {
        self.gate.with_extension("waiting")
    }

    pub(crate) fn status(&self, id: &str) -> Option<String> {
        self.query("SELECT status FROM tasks WHERE id = ?1", id)
    }

    pub(crate) fn child_of(&self, parent: &str) -> Option<String> {
        self.query("SELECT id FROM tasks WHERE parent_task_id = ?1", parent)
    }

    fn query(&self, sql: &str, arg: &str) -> Option<String> {
        let conn = Connection::open(self.home().join("aid.db")).ok()?;
        conn.query_row(sql, [arg], |row| row.get::<_, String>(0)).ok()
    }

    /// Poll `probe` until it yields a value or `timeout` elapses.
    pub(crate) fn wait_for<T>(&self, timeout: Duration, probe: impl Fn(&Self) -> Option<T>) -> T {
        let deadline = Instant::now() + timeout;
        loop {
            if let Some(value) = probe(self) {
                return value;
            }
            assert!(Instant::now() < deadline, "condition not reached within {timeout:?}");
            thread::sleep(Duration::from_millis(50));
        }
    }

    pub(crate) fn wait_terminal(&self, id: &str) -> String {
        self.wait_for(RELEASE_WAIT, |h| h.status(id).filter(|s| TERMINAL.contains(&s.as_str())))
    }
}

impl Drop for Harness {
    fn drop(&mut self) {
        // Never leave a verifier holding past a failed assertion.
        let _ = std::fs::write(&self.gate, "1");
    }
}

// Commits a file when run inside a git checkout so worktree runs deliver a real change.
const AGENT_SCRIPT: &str = "#!/bin/sh\n\
printf 'settled\\n' > settled.txt\n\
if git rev-parse --git-dir >/dev/null 2>&1; then\n\
  git add settled.txt && git -c user.name=t -c user.email=t@example.com commit -qm settle\n\
fi\n\
printf '%s\\n' '{\"type\":\"completion\",\"finalText\":\"done\"}'\n";

fn write_custom_agent(aid_home: &Path, script: &Path) {
    let agents_dir = aid_home.join("agents");
    std::fs::create_dir_all(&agents_dir).unwrap();
    std::fs::write(
        agents_dir.join(format!("{AGENT}.toml")),
        format!(
            "[agent]\nid = \"{AGENT}\"\ndisplay_name = \"{AGENT}\"\ncommand = \"{}\"\n\
             trust_tier = \"local\"\nstreaming = true\noutput_format = \"jsonl\"\n",
            script.display(),
        ),
    )
    .unwrap();
}

fn write_script(dir: &Path, name: &str, contents: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, contents).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o755)).unwrap();
    path
}
