// Positive-control support for caller-owned recovery acceptance pairs.
// Isolates executable discovery, counts recovery children, and inspects public show events.
// Deps: caller_owned harness, CLI JSON, POSIX utility symlinks and git.

use crate::caller_owned::{Harness, MODEL, identity, json};
use serde_json::Value;
use std::fs;
use std::process::Command;

pub struct Recovery {
    pub h: Harness,
}

impl Recovery {
    pub fn new(body: &str) -> Self {
        let h = Harness::new(body);
        fs::create_dir(h.path("tools")).unwrap();
        // No provider executable is inherited, even if a recovery gate regresses.
        let search = std::env::var_os("PATH").unwrap();
        for name in [
            "sh", "bash", "git", "ps", "sleep", "touch", "cat", "rm", "mkdir", "dirname", "basename", "date",
            "kill", "uname", "head", "tail", "wc", "cut", "tr", "sed", "awk", "readlink", "realpath", "cp",
            "mv", "chmod", "env",
        ] {
            let source = std::env::split_paths(&search)
                .map(|dir| dir.join(name))
                .find(|p| p.is_file());
            if let Some(source) = source {
                std::os::unix::fs::symlink(fs::canonicalize(source).unwrap(), h.path("tools").join(name))
                    .unwrap();
            }
        }
        for essential in ["sh", "git", "ps", "sleep", "touch"] {
            assert!(
                h.path("tools").join(essential).is_file(),
                "missing fixture tool {essential}"
            );
        }
        Self { h }
    }

    pub fn command(&self, mut cmd: Command) -> Command {
        cmd.env("PATH", self.h.path("tools"))
            // timeout_policy::resolve reads this before persisting the worker environment.
            .env("AID_FIRST_TOKEN_TIMEOUT_SECS", "1")
            .env("GIT_AUTHOR_NAME", "fixture")
            .env("GIT_COMMITTER_NAME", "fixture")
            .env("GIT_AUTHOR_EMAIL", "fixture@example.test")
            .env("GIT_COMMITTER_EMAIL", "fixture@example.test");
        cmd
    }

    pub fn start(&self, id: &str, owned: bool, extra: &[&str]) {
        let template = self.h.run(id, extra);
        let mut cmd = if owned {
            template
        } else {
            let args: Vec<_> = template.get_args().map(|s| s.to_os_string()).collect();
            let mut cmd = self.h.aid();
            let mut index = 0;
            while index < args.len() {
                if args[index] == "--policy" {
                    index += 2;
                } else if args[index] == "--json" {
                    index += 1;
                } else {
                    cmd.arg(&args[index]);
                    index += 1;
                }
            }
            cmd
        };
        cmd = self.command(cmd);
        if owned {
            let receipt = json(cmd, 0);
            identity(&receipt, id);
            assert_eq!(receipt["submission"], "launched");
        } else {
            let out = crate::caller_owned::bounded(&mut cmd);
            assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        }
    }

    pub fn show(&self, id: &str) -> Value {
        let mut cmd = self.h.aid();
        cmd.args(["show", id, "--json"]);
        json(self.command(cmd), 0)
    }

    pub fn settle(&self, id: &str) -> Value {
        if self.show(id)["execution"]["policy"].is_null() {
            let mut cmd = self.h.aid();
            cmd.args(["wait", id, "--timeout", "25"]);
            let out = crate::caller_owned::bounded(&mut self.command(cmd));
            assert!(
                matches!(out.status.code(), Some(0 | 1)),
                "default wait failed: {}",
                String::from_utf8_lossy(&out.stderr)
            );
            let receipt = self.show(id)["execution"].clone();
            assert_eq!(receipt["task"]["terminal"], true);
            assert_eq!(receipt["worker"]["state"], "unconfirmed");
            assert_eq!(receipt["settled"], false);
            return receipt;
        }
        let mut cmd = self.h.aid();
        cmd.args(["wait", id, "--json", "--timeout", "25"]);
        let receipt = json(self.command(cmd), 0);
        assert_eq!(receipt["settled"], true);
        assert_eq!(receipt["task"]["terminal"], true);
        receipt
    }

    pub fn rows(&self) -> Vec<Value> {
        let mut cmd = self.h.aid();
        cmd.args(["board", "--json"]);
        json(self.command(cmd), 0).as_array().unwrap().clone()
    }

    pub fn assert_control(&self, id: &str, invocations: usize, row_count: usize) {
        let receipt = self.settle(id);
        assert_eq!(receipt["task_id"], id);
        assert_eq!(receipt["task"]["requested_model"], MODEL);
        assert!(receipt["policy"].is_null());
        let rows = self.rows();
        assert_eq!(rows.len(), row_count, "{rows:?}");
        for row in &rows {
            self.settle(row["id"].as_str().unwrap());
        }
        assert_eq!(self.h.count(), invocations);
        if row_count == 2 {
            let child = rows.iter().find(|r| r["id"] != id).unwrap();
            let shown = self.show(child["id"].as_str().unwrap());
            assert_eq!(shown["parent_task_id"], id, "recovery must belong to this parent");
        }
    }

    pub fn assert_child_prompt(&self, id: &str, needle: &str) {
        let rows = self.rows();
        let child = rows.iter().find(|row| row["id"] != id).unwrap();
        let shown = self.show(child["id"].as_str().unwrap());
        assert!(shown["prompt"].as_str().unwrap().contains(needle), "{shown}");
    }

    pub fn assert_owned(&self, id: &str) -> Value {
        let receipt = self.settle(id);
        identity(&receipt, id);
        self.h.assert_rows(id, 1);
        let events = self.show(id)["events"].as_array().unwrap().clone();
        assert!(!events.iter().any(is_recovery_event), "recovery ran: {events:?}");
        receipt
    }

    pub fn has_event(&self, id: &str, needle: &str) -> bool {
        self.show(id)["events"]
            .as_array()
            .unwrap()
            .iter()
            .any(|event| event["detail"].as_str().unwrap_or_default().contains(needle))
    }

    pub fn git(&self, dir: &str, args: &[&str]) -> String {
        let mut cmd = Command::new("git");
        cmd.current_dir(dir).args(args);
        let out = crate::caller_owned::bounded(&mut self.command(cmd));
        assert!(out.status.success(), "{}", String::from_utf8_lossy(&out.stderr));
        String::from_utf8(out.stdout).unwrap()
    }
}

fn is_recovery_event(event: &Value) -> bool {
    // Rescue/checkpoint commits preserve changes; only another agent execution is recovery.
    let metadata = &event["metadata"];
    metadata["model_self_healed"] == true
        || metadata["hung_auto_retried"] == true
        || metadata["delivery_recovery"] == "started"
        || [
            "model unavailable → retry",
            "HUNG → retry",
            "Retrying uncommitted",
            "auto-cascading",
            "auto-retrying",
        ]
        .iter()
        .any(|needle| event["detail"].as_str().unwrap_or_default().contains(needle))
}

pub fn first_attempt(body: &str) -> String {
    format!(
        "if [ -f \"$AID_HOME/attempted\" ]; then\n{}\nexit 0\nfi\ntouch \"$AID_HOME/attempted\"\n{body}",
        crate::caller_owned::SUCCESS,
    )
}
