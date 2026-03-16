// Persistence layer for experiment runs (append-only JSONL).
// Exports: save_run, load_state, experiment_file_path.
// Deps: serde_json, std::fs, crate::cmd::experiment_types.
use crate::cmd::experiment_types::{ExperimentConfig, ExperimentRun, ExperimentState};
#[cfg(test)]
use crate::cmd::experiment_types::MetricDirection;
use anyhow::Result;
use std::fs::{self, File};
use std::io::{BufRead, BufReader, Write};
use std::path::Path;

pub fn experiment_file_path(dir: &str) -> std::path::PathBuf {
    Path::new(dir).join("experiment.jsonl")
}

pub fn save_run(path: &Path, run: &ExperimentRun) -> Result<()> {
    let mut file = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)?;
    writeln!(file, "{}", serde_json::to_string(run)?)?;
    Ok(())
}

pub fn load_state(path: &Path, config: &ExperimentConfig) -> Result<ExperimentState> {
    let mut state = ExperimentState::new(config.clone());
    if !path.exists() {
        return Ok(state);
    }
    for line in BufReader::new(File::open(path)?).lines().map_while(Result::ok) {
        if !line.trim().is_empty()
            && let Ok(run) = serde_json::from_str::<ExperimentRun>(&line) {
                state.record_run(run);
            }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn test_cfg() -> ExperimentConfig {
        ExperimentConfig {
            metric_command: "echo 1".into(), direction: MetricDirection::Max,
            agent: "test".into(), prompt: "test".into(),
            checks: None, max_runs: Some(5), worktree: None, verify: None,
        }
    }

    fn mk_run(id: usize, kept: bool) -> ExperimentRun {
        ExperimentRun {
            run_id: id, task_id: format!("t-{id}"), agent: "test".into(),
            metric_value: Some(id as f64), checks_passed: Some(true),
            kept, timestamp: "2026-03-16".into(), duration_ms: Some(100),
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("experiment.jsonl");
        save_run(&path, &mk_run(1, true)).unwrap();
        save_run(&path, &mk_run(2, false)).unwrap();
        let state = load_state(&path, &test_cfg()).unwrap();
        assert_eq!(state.runs.len(), 2);
        assert_eq!(state.runs[0].run_id, 1);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = std::path::Path::new("/nonexistent/experiment.jsonl");
        assert!(load_state(path, &test_cfg()).unwrap().runs.is_empty());
    }
}
