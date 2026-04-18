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
    let current_dir = std::env::current_dir().ok();
    parse_batch_file_with_vars_and_source(path, &HashMap::new(), Some(path), current_dir.as_deref())
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
    let current_dir = std::env::current_dir().ok();
    parse_batch_file_with_vars_and_source(path, cli_vars, Some(path), current_dir.as_deref())
}

pub(crate) fn parse_batch_file_with_vars_and_source(
    path: &Path,
    cli_vars: &HashMap<String, String>,
    source_path: Option<&Path>,
    pwd: Option<&Path>,
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
    let batch_dir = batch_source_dir(source_path, pwd);
    resolve_batch_paths(&mut config.tasks, batch_dir.as_deref(), pwd)?;
    resolve_task_prompts(&mut config.tasks, batch_dir.as_deref(), pwd)?;
    validate_agents(&config.tasks)?;
    validate_fallback_agents(&config.tasks)?;
    auto_sequence_shared_worktrees(&mut config.tasks, &mut stderr)?;
    warn_prompt_size(&config.tasks, &mut stderr)?;
    validate_dag(&config.tasks)?;
    validate_conditional_hooks(&config.tasks)?;
    warn_audit_without_readonly(&config.tasks);
    Ok(config)
}

fn batch_source_dir(source_path: Option<&Path>, pwd: Option<&Path>) -> Option<PathBuf> {
    let parent = source_path?.parent()?;
    if parent.as_os_str().is_empty() {
        return None;
    }
    Some(if parent.is_absolute() {
        parent.to_path_buf()
    } else if let Some(pwd) = pwd {
        pwd.join(parent)
    } else {
        parent.to_path_buf()
    })
}

fn resolve_batch_paths(tasks: &mut [BatchTask], batch_dir: Option<&Path>, pwd: Option<&Path>) -> Result<()> {
    for task in tasks {
        if let Some(dir) = task.dir.as_mut() {
            resolve_batch_path("dir", dir, batch_dir, pwd)?;
        }
        if let Some(context) = task.context.as_mut() {
            for entry in context {
                resolve_batch_context_path(entry, batch_dir, pwd)?;
            }
        }
    }
    Ok(())
}

fn resolve_batch_path(field: &str, value: &mut String, batch_dir: Option<&Path>, pwd: Option<&Path>) -> Result<()> {
    let raw_value = value.clone();
    let path = Path::new(&raw_value);
    if !raw_value.is_empty() && path.is_relative() {
        *value = resolve_relative_batch_path(field, &raw_value, path, None, batch_dir, pwd)?;
    }
    Ok(())
}

fn resolve_batch_context_path(value: &mut String, batch_dir: Option<&Path>, pwd: Option<&Path>) -> Result<()> {
    let raw_value = value.clone();
    let (file, item) = match raw_value.split_once(':') {
        Some((file, item)) => (file, Some(item)),
        None => (raw_value.as_str(), None),
    };
    let path = Path::new(file);
    if !file.is_empty() && path.is_relative() {
        *value = resolve_relative_batch_path("context", &raw_value, path, item, batch_dir, pwd)?;
    }
    Ok(())
}

fn resolve_relative_batch_path(
    field: &str,
    raw_value: &str,
    relative_path: &Path,
    item: Option<&str>,
    batch_dir: Option<&Path>,
    pwd: Option<&Path>,
) -> Result<String> {
    if let Some(candidate) = batch_dir.map(|dir| dir.join(relative_path))
        && candidate.exists()
    {
        return Ok(with_context_item(candidate, item));
    }
    if let Some(candidate) = pwd.map(|dir| dir.join(relative_path))
        && candidate.exists()
    {
        return Ok(with_context_item(candidate, item));
    }

    let toml_attempt = attempted_path(batch_dir, relative_path, "<toml-dir unavailable>");
    let pwd_attempt = attempted_path(pwd, relative_path, "<pwd unavailable>");
    anyhow::bail!(
        "{field} = '{}' in batch TOML could not be resolved: tried {toml_attempt} and {pwd_attempt} — use an absolute path or place the TOML inside the target repo",
        raw_value
    );
}

fn attempted_path(base_dir: Option<&Path>, relative_path: &Path, fallback: &str) -> String {
    match base_dir {
        Some(dir) => dir.join(relative_path).display().to_string(),
        None => format!("{fallback}/{}", relative_path.display()),
    }
}

fn with_context_item(path: PathBuf, item: Option<&str>) -> String {
    let normalized: PathBuf = path.components().collect();
    match item {
        Some(item) => format!("{}:{item}", normalized.to_string_lossy()),
        None => normalized.to_string_lossy().into_owned(),
    }
}

fn resolve_task_prompts(tasks: &mut [BatchTask], batch_dir: Option<&Path>, pwd: Option<&Path>) -> Result<()> {
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
                let prompt_path = batch_prompt_path(batch_dir, pwd, file)?;
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

fn batch_prompt_path(batch_dir: Option<&Path>, pwd: Option<&Path>, file: &str) -> Result<PathBuf> {
    let path = Path::new(file);
    if path.is_absolute() {
        Ok(path.to_path_buf())
    } else {
        Ok(PathBuf::from(resolve_relative_batch_path(
            "prompt_file",
            file,
            path,
            None,
            batch_dir,
            pwd,
        )?))
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
