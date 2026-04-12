// Batch task file parser: reads TOML batch configs and validates task DAGs.
// Each batch file declares tasks with agent, prompt, overrides, and dependencies.
#[path = "batch_interpolate.rs"]
mod batch_interpolate;
#[path = "batch_legacy_fields.rs"]
mod batch_legacy_fields;
#[path = "batch_serde.rs"]
mod batch_serde;
#[path = "batch/schema.rs"]
mod schema;
#[path = "batch/validate.rs"]
mod validate;
#[path = "batch/warnings.rs"]
mod warnings;

use self::batch_interpolate::{apply_defaults, interpolate_batch_config};
use self::batch_legacy_fields::validate_legacy_field_renames;
use self::validate::{
    validate_agents, validate_conditional_hooks, validate_dag, validate_fallback_agents,
};
use self::warnings::warn_prompt_size;
use anyhow::{Context, Result};
use std::collections::HashMap;
use std::io;
use std::path::{Path, PathBuf};

pub use self::schema::{BatchConfig, BatchDefaults, BatchTask};
pub use self::validate::auto_sequence_shared_worktrees;
pub(crate) use self::validate::{dependency_indices, is_valid_agent, task_name_map};
#[cfg(test)]
pub(crate) use self::warnings::warn_audit_without_readonly_into;

pub fn warn_audit_without_readonly(tasks: &[BatchTask]) {
    self::warnings::warn_audit_without_readonly(tasks);
}

#[cfg_attr(not(test), allow(dead_code))]
pub fn warn_dir_overlap(tasks: &[BatchTask]) -> Vec<String> {
    self::warnings::warn_dir_overlap(tasks)
}

pub fn parse_batch_file(path: &Path) -> Result<BatchConfig> {
    parse_batch_file_with_vars(path, &HashMap::new())
}

fn validate_batch_keys(content: &str, path: &Path) -> Result<()> {
    let raw: toml::Value = toml::from_str(content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    let Some(table) = raw.as_table() else {
        return Ok(());
    };
    let known_keys = ["title", "description", "defaults", "tasks", "task", "vars"];
    for key in table.keys() {
        if known_keys.contains(&key.as_str()) {
            continue;
        }
        let suggestion = match key.as_str() {
            "default" => " (did you mean `[defaults]`?)",
            _ => "",
        };
        anyhow::bail!(
            "unknown top-level key `{}` in batch file {}{}",
            key,
            path.display(),
            suggestion
        );
    }
    Ok(())
}

pub(crate) fn parse_batch_file_with_vars(
    path: &Path,
    cli_vars: &HashMap<String, String>,
) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
    validate_batch_keys(&content, path)?;
    validate_legacy_field_renames(&content, path)?;
    let mut config: BatchConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    if config.tasks.is_empty() {
        anyhow::bail!("batch file contains no tasks");
    }
    let mut stderr = io::stderr().lock();
    interpolate_batch_config(&mut config, cli_vars, &mut stderr)?;
    apply_defaults(&mut config.tasks, &config.defaults);
    resolve_batch_paths(&mut config.tasks, path);
    resolve_task_prompts(&mut config.tasks, path)?;
    validate_agents(&config.tasks)?;
    validate_fallback_agents(&config.tasks)?;
    auto_sequence_shared_worktrees(&mut config.tasks, &mut stderr)?;
    warn_prompt_size(&config.tasks, &mut stderr)?;
    validate_dag(&config.tasks)?;
    validate_conditional_hooks(&config.tasks)?;
    warn_audit_without_readonly(&config.tasks);
    Ok(config)
}

fn resolve_batch_paths(tasks: &mut [BatchTask], batch_path: &Path) {
    let base_dir = batch_path.parent().unwrap_or_else(|| Path::new("."));
    for task in tasks {
        if let Some(dir) = task.dir.as_mut() {
            resolve_batch_path(base_dir, dir);
        }
        if let Some(context) = task.context.as_mut() {
            for entry in context {
                resolve_batch_path(base_dir, entry);
            }
        }
    }
}

fn resolve_batch_path(base_dir: &Path, value: &mut String) {
    let path = Path::new(value);
    if !value.is_empty() && path.is_relative() {
        *value = base_dir.join(path).to_string_lossy().into_owned();
    }
}

fn resolve_task_prompts(tasks: &mut [BatchTask], batch_path: &Path) -> Result<()> {
    let base_dir = batch_path.parent().unwrap_or_else(|| Path::new("."));
    for (task_idx, task) in tasks.iter_mut().enumerate() {
        let has_prompt = !task.prompt.trim().is_empty();
        match (task.prompt_file.as_deref(), has_prompt) {
            (Some(_), true) => anyhow::bail!(
                "task {} cannot set both prompt and prompt_file",
                task_label(task, task_idx)
            ),
            (None, false) => anyhow::bail!(
                "task {} must set either prompt or prompt_file",
                task_label(task, task_idx)
            ),
            (Some(file), false) => {
                let prompt_path = batch_prompt_path(base_dir, file);
                task.prompt = std::fs::read_to_string(&prompt_path).with_context(|| {
                    format!(
                        "failed to read prompt file for task {}: {}",
                        task_label(task, task_idx),
                        prompt_path.display()
                    )
                })?;
            }
            (None, true) => {}
        }
    }
    Ok(())
}

fn batch_prompt_path(base_dir: &Path, file: &str) -> PathBuf {
    let path = Path::new(file);
    if path.is_absolute() {
        path.to_path_buf()
    } else {
        base_dir.join(path)
    }
}

fn task_label(task: &BatchTask, task_idx: usize) -> String {
    task.name.clone().unwrap_or_else(|| format!("#{task_idx}"))
}
#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "batch/legacy_field_tests.rs"]
mod legacy_field_tests;

#[cfg(test)]
#[path = "batch/max_concurrent_tests.rs"]
mod max_concurrent_tests;
