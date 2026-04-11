// Run argument types and lightweight helpers for `aid run`.
// Exports: RunArgs, NO_SKILL_SENTINEL, prompt/timeout resolution helpers.
// Deps: anyhow, crate::types::TaskId, std collections.
use anyhow::{Context, Result};
use crate::types::TaskId;
use std::collections::HashMap;

pub const NO_SKILL_SENTINEL: &str = "__aid_no_skill__";

#[derive(Clone)]
pub struct RunArgs {
    pub agent_name: String,
    pub prompt: String,
    pub prompt_file: Option<String>,
    pub repo: Option<String>,
    pub repo_root: Option<String>,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub result_file: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub base_branch: Option<String>,
    pub group: Option<String>,
    pub verify: Option<String>,
    pub setup: Option<String>,
    pub iterate: Option<u32>,
    pub eval: Option<String>,
    pub eval_feedback_template: Option<String>,
    pub judge: Option<String>,
    pub peer_review: Option<String>,
    pub max_duration_mins: Option<i64>,
    pub max_task_cost: Option<f64>,
    pub retry: u32,
    pub context: Vec<String>,
    pub checklist: Vec<String>,
    pub skills: Vec<String>,
    pub hooks: Vec<String>,
    pub template: Option<String>,
    pub background: bool,
    pub dry_run: bool,
    pub announce: bool,
    pub parent_task_id: Option<String>,
    pub on_done: Option<String>,
    pub cascade: Vec<String>,
    pub read_only: bool,
    pub sandbox: bool,
    pub container: Option<String>,
    pub budget: bool,
    pub best_of: Option<usize>,
    pub metric: Option<String>,
    pub session_id: Option<String>,
    pub team: Option<String>,
    pub context_from: Vec<String>,
    pub batch_siblings: Vec<(String, String, String)>,
    pub scope: Vec<String>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
    pub judge_retry: bool,
    pub existing_task_id: Option<TaskId>,
    pub timeout: Option<u64>,
    pub suppress_nested_repo_warning: bool,
    pub link_deps: bool,
}

impl Default for RunArgs {
    fn default() -> Self {
        Self {
            agent_name: String::new(),
            prompt: String::new(),
            prompt_file: None,
            repo: None,
            repo_root: None,
            dir: None,
            output: None,
            result_file: None,
            model: None,
            worktree: None,
            base_branch: None,
            group: None,
            verify: None,
            setup: None,
            iterate: None,
            eval: None,
            eval_feedback_template: None,
            judge: None,
            peer_review: None,
            max_duration_mins: None,
            max_task_cost: None,
            retry: 0,
            context: Vec::new(),
            checklist: Vec::new(),
            skills: Vec::new(),
            hooks: Vec::new(),
            template: None,
            background: false,
            dry_run: false,
            announce: false,
            parent_task_id: None,
            on_done: None,
            cascade: Vec::new(),
            read_only: false,
            sandbox: false,
            container: None,
            budget: false,
            best_of: None,
            metric: None,
            session_id: None,
            team: None,
            context_from: Vec::new(),
            batch_siblings: Vec::new(),
            scope: Vec::new(),
            env: None,
            env_forward: None,
            judge_retry: false,
            existing_task_id: None,
            timeout: None,
            suppress_nested_repo_warning: false,
            link_deps: true,
        }
    }
}

pub(crate) fn resolve_max_duration_mins(
    timeout: Option<u64>,
    max_duration_mins: Option<i64>,
) -> Option<i64> {
    max_duration_mins.or_else(|| timeout.map(|secs| secs.div_ceil(60) as i64))
}

pub(crate) fn resolve_prompt_input(prompt: &str, prompt_file: Option<&str>) -> Result<String> {
    match (prompt_file, prompt) {
        (Some(file), "") => std::fs::read_to_string(file)
            .with_context(|| format!("Failed to read prompt file: {file}")),
        (None, prompt) if !prompt.is_empty() => Ok(prompt.to_string()),
        (Some(_), _) => anyhow::bail!("Cannot use both --prompt and --prompt-file"),
        (None, _) => anyhow::bail!("Either prompt or --prompt-file is required"),
    }
}

pub(super) fn preview_prompt(prompt: &str, max_chars: usize) -> String {
    let mut preview: String = prompt.chars().take(max_chars).collect();
    if prompt.chars().count() > max_chars {
        preview.push_str("...");
    }
    preview
}

pub(super) fn context_file_from_spec(spec: &str) -> String {
    spec.split_once(':')
        .map_or_else(|| spec.to_string(), |(file, _)| file.to_string())
}
