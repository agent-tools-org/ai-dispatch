// Batch parser tests covering TOML parsing, validation, and defaults resolution.
// Exports: module-local tests only.
// Deps: super::parse_batch_file, super::validate_dag, tempfile::NamedTempFile

use super::*;
use std::io::Write;
use std::path::{Path, PathBuf};
use tempfile::{NamedTempFile, tempdir};

fn write_temp(content: &str) -> NamedTempFile {
    let mut file = NamedTempFile::new().unwrap();
    file.write_all(content.as_bytes()).unwrap();
    file.flush().unwrap();
    file
}

fn write_batch_file(dir: &Path, name: &str, content: &str) -> PathBuf {
    let path = dir.join(name);
    std::fs::write(&path, content).unwrap();
    path
}

fn parse_batch_with_vars(content: &str, cli_vars: &[(&str, &str)]) -> (BatchConfig, String) {
    let vars = cli_vars
        .iter()
        .map(|(key, value)| (key.to_string(), value.to_string()))
        .collect();
    let mut stderr = Vec::new();
    let mut config = toml::from_str::<BatchConfig>(content).unwrap();
    interpolate_batch_config(&mut config, &vars, &mut stderr).unwrap();
    apply_defaults(&mut config.tasks, &config.defaults);
    (config, String::from_utf8(stderr).unwrap())
}

fn make_task(name: Option<&str>, depends_on: &[&str]) -> BatchTask {
    BatchTask {
        id: None,
        name: name.map(str::to_string),
        agent: "codex".to_string(),
        team: None,
        prompt: "prompt".to_string(),
        prompt_file: None,
        dir: None,
        output: None,
        result_file: None,
        model: None,
        worktree: None,
        group: None,
        container: None,
        remote_build: None,
        best_of: None,
        max_duration_mins: None,
        max_wait_mins: None,
        retry: None,
        iterate: None,
        eval: None,
        eval_feedback_template: None,
        idle_timeout: None,
        verify: None,
        setup: None,
        judge: None,
        peer_review: None,
        metric: None,
        context: None,
        checklist: None,
        skills: None,
        on_done: None,
        hooks: None,
        depends_on: (!depends_on.is_empty())
            .then(|| depends_on.iter().map(|item| item.to_string()).collect()),
        parent: None,
        context_from: None,
        fallback: None,
        scope: None,
        read_only: false,
        sandbox: false,
        no_skill: false,
        difficulty: None, budget: None, urgency: None, rigor: None, egress: None, kind: None,
        audit: None,
            env: None,
        env_forward: None,
        worktree_link_deps: None,
        on_success: None,
        on_fail: None,
        conditional: false,
    }
}

#[path = "parser_cases_1.rs"]
mod parser_cases_1;
#[path = "parser_cases_2.rs"]
mod parser_cases_2;
#[path = "parser_cases_3.rs"]
mod parser_cases_3;
#[path = "parser_cases_4.rs"]
mod parser_cases_4;
#[path = "parser_cases_5.rs"]
mod parser_cases_5;
