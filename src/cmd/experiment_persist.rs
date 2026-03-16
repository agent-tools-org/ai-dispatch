// Persistence layer for experiment runs (append-only JSONL).
// Exports: save_run, load_state, experiment_file_path.
// Deps: serde_json, std::fs, crate::cmd::experiment_types.
use crate::cmd::experiment_types::{ExperimentConfig, ExperimentRun, ExperimentState};
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
    writeln!(file, "{}", serde_json::to_string(run)?);
    Ok(())
}

pub fn load_state(path: &Path, config: &ExperimentConfig) -> Result<ExperimentState> {
    let mut state = ExperimentState::new(config.clone());
    if !path.exists() {
        return Ok(state);
    }
    for line in BufReader::new(File::open(path)?).lines().flatten() {
        if !line.trim().is_empty() {
            if let Ok(run) = serde_json::from_str::<ExperimentRun>(&line) {
                state.record_run(run);
            }
        }
    }
    Ok(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;
    fn mk_run(i: u32, s: bool, d: u64) -> ExperimentRun {
        ExperimentRun {
            iteration: i,
            success: s,
            duration_ms: d,
        }
    }

    #[test]
    fn save_and_load_roundtrip() {
        let dir = TempDir::new().unwrap();
        let path = dir.path().join("experiment.jsonl");
        let cfg = ExperimentConfig {
            name: "test".into(),
            iterations: 10,
        };
        save_run(&path, &mk_run(1, true, 100)).unwrap();
        save_run(&path, &mk_run(2, false, 200)).unwrap();
        let state = load_state(&path, &cfg).unwrap();
        assert_eq!(state.runs.len(), 2);
        assert_eq!(state.runs[0].iteration, 1);
    }

    #[test]
    fn load_missing_file_returns_empty() {
        let path = std::path::Path::new("/nonexistent/experiment.jsonl");
        let cfg = ExperimentConfig {
            name: "test".into(),
            iterations: 5,
        };
        assert!(load_state(&path, &cfg).unwrap().runs.is_empty());
    }
}
