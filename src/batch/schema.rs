// Batch TOML schema types for defaults and per-task overrides.
// Exports: BatchConfig, BatchDefaults, BatchTask.
// Deps: serde, std collections, and batch serde helpers.

use serde::Deserialize;
use std::collections::HashMap;

use super::batch_serde::{deserialize_judge, deserialize_string_or_vec, deserialize_verify};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    #[serde(default)]
    pub title: Option<String>,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub defaults: BatchDefaults,
    #[serde(default)]
    pub vars: HashMap<String, String>,
    #[serde(alias = "task", alias = "tasks")]
    pub tasks: Vec<BatchTask>,
}

#[derive(Debug, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BatchDefaults {
    pub group_id: Option<String>,
    pub group: Option<String>,
    #[serde(default)]
    pub shared_dir: Option<bool>,
    #[serde(default)]
    pub analyze: Option<bool>,
    pub agent: Option<String>,
    #[serde(default)]
    pub auto_fallback: Option<bool>,
    pub team: Option<String>,
    pub dir: Option<String>,
    pub repo_root: Option<String>,
    pub model: Option<String>,
    pub worktree_prefix: Option<String>,
    #[serde(default, deserialize_with = "deserialize_judge")]
    pub judge: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default)]
    pub peer_review: Option<String>,
    #[serde(default)]
    pub container: Option<String>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    #[serde(default)]
    pub max_wait_mins: Option<u64>,
    #[serde(default)]
    pub max_concurrent: Option<usize>,
    #[serde(default)]
    pub retry: Option<u32>,
    #[serde(default)]
    pub iterate: Option<u32>,
    #[serde(default)]
    pub eval: Option<String>,
    #[serde(default)]
    pub eval_feedback_template: Option<String>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default)]
    pub best_of: Option<usize>,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub on_done: Option<String>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub scope: Option<Vec<String>>,
    #[serde(default)]
    pub read_only: Option<bool>,
    #[serde(default)]
    pub sandbox: Option<bool>,
    #[serde(default)]
    pub no_skill: Option<bool>,
    #[serde(default)]
    pub budget: Option<bool>,
    #[serde(default)]
    pub audit: Option<bool>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub env_forward: Option<Vec<String>>,
    #[serde(default)]
    pub worktree_link_deps: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchTask {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub agent: String,
    pub team: Option<String>,
    #[serde(default)]
    pub prompt: String,
    #[serde(default)]
    pub prompt_file: Option<String>,
    pub dir: Option<String>,
    pub output: Option<String>,
    #[serde(default)]
    pub result_file: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub group: Option<String>,
    pub container: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    #[serde(default)]
    pub setup: Option<String>,
    #[serde(default, deserialize_with = "deserialize_judge")]
    pub judge: Option<String>,
    #[serde(default)]
    pub peer_review: Option<String>,
    #[serde(default)]
    pub best_of: Option<usize>,
    #[serde(default)]
    pub max_duration_mins: Option<u64>,
    #[serde(default)]
    pub max_wait_mins: Option<u64>,
    #[serde(default)]
    pub retry: Option<u32>,
    #[serde(default)]
    pub iterate: Option<u32>,
    #[serde(default)]
    pub eval: Option<String>,
    #[serde(default)]
    pub eval_feedback_template: Option<String>,
    #[serde(default)]
    pub idle_timeout: Option<u64>,
    #[serde(default)]
    pub metric: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    #[serde(default)]
    pub checklist: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    #[serde(default)]
    pub on_done: Option<String>,
    #[serde(default)]
    pub hooks: Option<Vec<String>>,
    pub depends_on: Option<Vec<String>>,
    pub parent: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context_from: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub scope: Option<Vec<String>>,
    #[serde(default)]
    pub read_only: bool,
    #[serde(default)]
    pub sandbox: bool,
    #[serde(default)]
    pub no_skill: bool,
    #[serde(default)]
    pub budget: bool,
    #[serde(default)]
    pub audit: Option<bool>,
    #[serde(default)]
    pub env: Option<HashMap<String, String>>,
    #[serde(default)]
    pub env_forward: Option<Vec<String>>,
    #[serde(default)]
    pub worktree_link_deps: Option<bool>,
    pub on_success: Option<String>,
    pub on_fail: Option<String>,
    #[serde(default)]
    pub conditional: bool,
}
