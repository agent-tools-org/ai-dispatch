// Batch TOML schema types for defaults and per-task overrides.
// Exports: BatchConfig, BatchDefaults, BatchTask.
// Deps: serde, std collections, and batch serde helpers.

use serde::Deserialize;
use std::collections::HashMap;

use crate::agent::classifier::TaskCategory;
use crate::types::{TaskBudget, TaskDifficulty, TaskEgress, TaskRigor, TaskUrgency};
use super::serde_fields::{deserialize_judge, deserialize_string_or_vec, deserialize_verify};

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BatchConfig {
    pub title: Option<String>,
    #[serde(default)]
    #[allow(dead_code)] // Retained as part of the batch TOML schema.
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
    pub shared_dir: Option<bool>,
    pub analyze: Option<bool>,
    pub agent: Option<String>,
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
    pub setup: Option<String>,
    pub peer_review: Option<String>,
    pub container: Option<String>,
    pub remote_build: Option<String>,
    pub max_duration_mins: Option<u64>,
    pub max_wait_mins: Option<u64>,
    pub max_concurrent: Option<usize>,
    pub retry: Option<u32>,
    pub iterate: Option<u32>,
    pub eval: Option<String>,
    pub eval_feedback_template: Option<String>,
    pub idle_timeout: Option<u64>,
    pub best_of: Option<usize>,
    pub metric: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    pub on_done: Option<String>,
    pub hooks: Option<Vec<String>>,
    pub fallback: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub scope: Option<Vec<String>>,
    pub read_only: Option<bool>,
    pub sandbox: Option<bool>,
    pub no_skill: Option<bool>,
    pub difficulty: Option<TaskDifficulty>,
    pub budget: Option<TaskBudget>,
    pub urgency: Option<TaskUrgency>,
    pub rigor: Option<TaskRigor>,
    pub egress: Option<TaskEgress>,
    pub kind: Option<TaskCategory>,
    pub audit: Option<bool>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
    pub worktree_link_deps: Option<bool>,
}

#[derive(Debug, Deserialize)]
#[cfg_attr(test, derive(Default))]
#[serde(deny_unknown_fields)]
pub struct BatchTask {
    pub id: Option<String>,
    pub name: Option<String>,
    #[serde(default)]
    pub agent: String,
    pub team: Option<String>,
    #[serde(default)]
    pub prompt: String,
    pub prompt_file: Option<String>,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub result_file: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub group: Option<String>,
    pub container: Option<String>,
    pub remote_build: Option<String>,
    #[serde(default, deserialize_with = "deserialize_verify")]
    pub verify: Option<String>,
    pub setup: Option<String>,
    #[serde(default, deserialize_with = "deserialize_judge")]
    pub judge: Option<String>,
    pub peer_review: Option<String>,
    pub best_of: Option<usize>,
    pub max_duration_mins: Option<u64>,
    pub max_wait_mins: Option<u64>,
    pub retry: Option<u32>,
    pub iterate: Option<u32>,
    pub eval: Option<String>,
    pub eval_feedback_template: Option<String>,
    pub idle_timeout: Option<u64>,
    pub metric: Option<String>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub context: Option<Vec<String>>,
    pub checklist: Option<Vec<String>>,
    #[serde(default, deserialize_with = "deserialize_string_or_vec")]
    pub skills: Option<Vec<String>>,
    pub on_done: Option<String>,
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
    pub difficulty: Option<TaskDifficulty>,
    pub budget: Option<TaskBudget>,
    pub urgency: Option<TaskUrgency>,
    pub rigor: Option<TaskRigor>,
    pub egress: Option<TaskEgress>,
    pub kind: Option<TaskCategory>,
    pub audit: Option<bool>,
    pub env: Option<HashMap<String, String>>,
    pub env_forward: Option<Vec<String>>,
    pub worktree_link_deps: Option<bool>,
    pub on_success: Option<String>,
    pub on_fail: Option<String>,
    #[serde(default)]
    pub conditional: bool,
}
