// Tests for `cmd::run_prompt` helpers and verify-failure retry behavior.
// Exports: none.
// Deps: run_prompt helpers, in-memory Store, temporary PATH/AID_HOME setup.

use super::*;
use crate::test_subprocess;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }

    fn remove(key: &'static str) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::remove_var(key) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn run_args(skills: Vec<String>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        skills,
        ..Default::default()
    }
}

fn build_prompt_args(output: Option<&str>, result_file: Option<&str>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Write the requested content".to_string(),
        output: output.map(str::to_string),
        result_file: result_file.map(str::to_string),
        ..Default::default()
    }
}


#[path = "tests_helpers.rs"]
mod helpers;

#[path = "tests_instructions.rs"]
mod instructions;

#[path = "tests_siblings.rs"]
mod siblings;
