// Batch task file parser: reads TOML batch configs for multi-task dispatch.
// Each batch file declares a list of tasks with agent, prompt, and optional overrides.

use anyhow::{Context, Result};
use serde::Deserialize;
use std::collections::HashSet;
use std::path::Path;

const VALID_AGENTS: &[&str] = &["gemini", "codex", "opencode", "cursor"];

#[derive(Debug, Deserialize)]
pub struct BatchConfig {
    #[serde(rename = "task")]
    pub tasks: Vec<BatchTask>,
}

#[derive(Debug, Deserialize)]
pub struct BatchTask {
    pub agent: String,
    pub prompt: String,
    pub dir: Option<String>,
    pub output: Option<String>,
    pub model: Option<String>,
    pub worktree: Option<String>,
    pub verify: Option<bool>,
}
pub fn parse_batch_file(path: &Path) -> Result<BatchConfig> {
    let content = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read batch file: {}", path.display()))?;
    let config: BatchConfig = toml::from_str(&content)
        .with_context(|| format!("failed to parse TOML in {}", path.display()))?;
    if config.tasks.is_empty() {
        anyhow::bail!("batch file contains no tasks");
    }
    for task in &config.tasks {
        if !VALID_AGENTS.contains(&task.agent.to_lowercase().as_str()) {
            anyhow::bail!("unknown agent: {}", task.agent);
        }
    }
    validate_no_file_overlap(&config.tasks)?;
    Ok(config)
}
pub fn validate_no_file_overlap(tasks: &[BatchTask]) -> Result<()> {
    let mut seen: HashSet<&str> = HashSet::new();
    for task in tasks {
        if let Some(ref wt) = task.worktree {
            if !seen.insert(wt.as_str()) {
                anyhow::bail!("duplicate worktree: {}", wt);
            }
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::NamedTempFile;
    fn write_temp(content: &str) -> NamedTempFile {
        let mut f = NamedTempFile::new().unwrap();
        f.write_all(content.as_bytes()).unwrap();
        f.flush().unwrap();
        f
    }
    #[test]
    fn parse_valid_batch() {
        let cfg = parse_batch_file(write_temp(concat!(
            "[[task]]\nagent = \"gemini\"\nprompt = \"research X\"\nworktree = \"feat/x\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"implement Y\"\ndir = \"src\"\nmodel = \"gpt-4\""
        )).path()).unwrap();
        assert_eq!(cfg.tasks.len(), 2);
        assert_eq!(cfg.tasks[0].agent, "gemini");
        assert_eq!(cfg.tasks[0].worktree, Some("feat/x".into()));
        assert_eq!(cfg.tasks[1].dir, Some("src".into()));
    }
    #[test]
    fn rejects_unknown_agent() {
        let f = write_temp("[[task]]\nagent = \"gpt-3\"\nprompt = \"do something\"");
        assert!(parse_batch_file(f.path())
            .unwrap_err()
            .to_string()
            .contains("unknown agent"));
    }
    #[test]
    fn rejects_duplicate_worktree() {
        let f = write_temp(concat!(
            "[[task]]\nagent = \"gemini\"\nprompt = \"a\"\nworktree = \"feat/x\"\n",
            "[[task]]\nagent = \"codex\"\nprompt = \"b\"\nworktree = \"feat/x\""
        ));
        assert!(parse_batch_file(f.path())
            .unwrap_err()
            .to_string()
            .contains("duplicate worktree"));
    }
    #[test]
    fn rejects_empty_batch() {
        let err = parse_batch_file(write_temp("").path())
            .unwrap_err()
            .to_string();
        assert!(err.contains("parse TOML") || err.contains("no tasks"));
    }
}
