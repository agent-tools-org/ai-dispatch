// Process-group fixture and liveness checks for caller-owned settlement tests.
// Keeps leader and child PID markers, bounded processes and failure cleanup together.
// Deps: caller_owned CLI harness, serde_json, libc and POSIX shell/ps.

use crate::caller_owned::{Harness, bounded, identity, json};

pub const GROUP_AGENT_SCRIPT: &str = concat!(
    "trap '' HUP PIPE\n",
    "(i=0; while [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done) &\n",
    "printf '%s\\n' \"$!\" > \"$AID_HOME/agent-child.pid.tmp\"\n",
    "mv \"$AID_HOME/agent-child.pid.tmp\" \"$AID_HOME/agent-child.pid\"\n",
    "printf '%s\\n' '{\"type\":\"message\",\"text\":\"holding\"}'\n",
    "printf '%s\\n' \"$$\" > \"$AID_HOME/agent.pid.tmp\"\n",
    "mv \"$AID_HOME/agent.pid.tmp\" \"$AID_HOME/agent.pid\"\n",
    "i=0\nwhile [ $i -lt 400 ]; do sleep 0.05; i=$((i+1)); done\n"
);

pub struct AgentProcess {
    pub pid: i32,
    armed: bool,
}

impl AgentProcess {
    pub fn from_marker(h: &Harness, name: &str) -> Self {
        let pid = std::fs::read_to_string(h.path(&format!("aid/{name}.pid")))
            .unwrap()
            .trim()
            .parse()
            .unwrap();
        Self { pid, armed: true }
    }

    pub fn assert_agent_command(&self) {
        let out = bounded(std::process::Command::new("ps").args([
            "-ww",
            "-p",
            &self.pid.to_string(),
            "-o",
            "args=",
        ]));
        let command = String::from_utf8_lossy(&out.stdout);
        assert!(
            out.status.success() && command.contains("controlled-agent"),
            "PID {} is not the controlled agent: {command}",
            self.pid
        );
    }

    pub fn is_alive(&self) -> bool {
        if unsafe { libc::kill(self.pid, 0) } != 0 {
            return false;
        }
        let out =
            bounded(std::process::Command::new("ps").args(["-p", &self.pid.to_string(), "-o", "stat="]));
        let state = String::from_utf8_lossy(&out.stdout);
        out.status.success() && !state.trim().is_empty() && !state.trim().starts_with('Z')
    }

    pub fn kill(&mut self) {
        assert_eq!(unsafe { libc::kill(self.pid, libc::SIGKILL) }, 0);
        self.armed = false;
    }
}

impl Drop for AgentProcess {
    fn drop(&mut self) {
        // Stop the deliberately orphaned agent even if a receipt assertion fails.
        if self.armed {
            unsafe {
                libc::kill(self.pid, libc::SIGKILL);
            }
        }
    }
}

fn assert_agent_unsettled(agent: &AgentProcess, receipt: &serde_json::Value, id: &str) {
    assert!(
        agent.is_alive(),
        "fixture process {} died while observing: {receipt}",
        agent.pid
    );
    identity(receipt, id);
    assert_eq!(receipt["settled"], false, "agent survived the worker: {receipt}");
    assert!(matches!(
        receipt["worker"]["state"].as_str(),
        Some("running" | "unconfirmed")
    ));
}

pub fn observe_unsettled(h: &Harness, process: &AgentProcess, id: &str) -> serde_json::Value {
    for _ in 0..3 {
        assert_agent_unsettled(process, &h.observe(id), id);
    }
    let shown = json(h.aid().args(["show", id, "--json"]), 0);
    assert_agent_unsettled(process, &shown["execution"], id);
    let waiting = json(h.aid().args(["wait", id, "--json", "--timeout", "1"]), 124);
    assert_agent_unsettled(process, &waiting, id);
    waiting
}
