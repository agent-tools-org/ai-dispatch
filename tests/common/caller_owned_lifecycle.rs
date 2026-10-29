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

pub fn assert_settlement_safe(
    receipt: &serde_json::Value,
    id: &str,
    agent: &AgentProcess,
    child: &AgentProcess,
    group: i32,
) {
    identity(receipt, id);
    if receipt["settled"] == true {
        assert!(
            !agent.is_alive(),
            "settled receipt left agent {} alive: {receipt}",
            agent.pid
        );
        assert!(
            !child.is_alive(),
            "settled receipt left child {} alive: {receipt}",
            child.pid
        );
        assert!(
            group_gone(group),
            "settled receipt left group {group} alive: {receipt}"
        );
    } else {
        assert!(matches!(
            receipt["worker"]["state"].as_str(),
            Some("running" | "unconfirmed")
        ));
    }
}

fn group_gone(group: i32) -> bool {
    let out = bounded(std::process::Command::new("ps").args(["-A", "-o", "pgid=,stat="]));
    assert!(out.status.success(), "could not inspect recorded process group");
    !String::from_utf8(out.stdout).unwrap().lines().any(|line| {
        let mut fields = line.split_whitespace();
        fields.next().and_then(|field| field.parse::<i32>().ok()) == Some(group)
            && fields
                .next()
                .is_some_and(|state| !state.starts_with('Z') && !state.starts_with('X'))
    })
}

pub fn observe_after_worker_loss(
    h: &Harness,
    id: &str,
    agent: &AgentProcess,
    child: &AgentProcess,
    group: i32,
) -> serde_json::Value {
    for _ in 0..3 {
        assert_settlement_safe(&h.observe(id), id, agent, child, group);
    }
    let shown = json(h.aid().args(["show", id, "--json"]), 0);
    assert_settlement_safe(&shown["execution"], id, agent, child, group);
    let (code, receipt) = crate::caller_owned::result(h.aid().args(["wait", id, "--json", "--timeout", "1"]));
    assert!(code == 0 || code == 124, "exit {code}: {receipt}");
    assert_eq!(code == 0, receipt["settled"] == true);
    assert_settlement_safe(&receipt, id, agent, child, group);
    receipt
}

pub fn complete_without_observation(h: &Harness, id: &str) {
    let verifier = h.verifier();
    h.launch(id, &["--verify", &verifier]);
    h.wait_for(|| h.path("holding").exists());
    let active = json(h.aid().args(["show", id, "--json"]), 0);
    let worker = active["execution"]["worker"]["pid"]
        .as_i64()
        .expect("gated worker PID") as i32;
    assert_eq!(active["execution"]["settled"], false);
    h.release();
    // No receipt observation: wait for process exit directly.
    h.wait_for(|| unsafe { libc::kill(worker, 0) } == -1);
}
