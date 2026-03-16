// Types for the autonomous experiment loop.
// Exports: ExperimentConfig, ExperimentRun, ExperimentResult.
// Deps: serde, chrono.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentConfig {
    pub metric_command: String,
    pub direction: MetricDirection,
    pub agent: String,
    pub prompt: String,
    pub checks: Option<String>,
    pub max_runs: Option<usize>,
    pub worktree: Option<String>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum MetricDirection { Min, Max }

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentRun {
    pub run_id: usize,
    pub task_id: String,
    pub agent: String,
    pub metric_value: Option<f64>,
    pub checks_passed: Option<bool>,
    pub kept: bool,
    pub timestamp: String,
    pub duration_ms: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExperimentState {
    pub config: ExperimentConfig,
    pub runs: Vec<ExperimentRun>,
    pub best_metric: Option<f64>,
    pub best_run_id: Option<usize>,
}

impl ExperimentState {
    pub fn new(config: ExperimentConfig) -> Self {
        Self { config, runs: Vec::new(), best_metric: None, best_run_id: None }
    }
    pub fn is_improvement(&self, value: f64) -> bool {
        match self.best_metric {
            None => true,
            Some(best) => match self.config.direction {
                MetricDirection::Min => value < best,
                MetricDirection::Max => value > best,
            },
        }
    }
    pub fn record_run(&mut self, run: ExperimentRun) {
        if run.kept && let Some(v) = run.metric_value {
            self.best_metric = Some(v);
            self.best_run_id = Some(run.run_id);
        }
        self.runs.push(run);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    fn cfg(d: MetricDirection) -> ExperimentConfig {
        ExperimentConfig { metric_command: "".into(), direction: d, agent: "".into(),
            prompt: "".into(), checks: None, max_runs: None, worktree: None }
    }
    #[test]
    fn is_improvement_max_direction() {
        assert!(ExperimentState::new(cfg(MetricDirection::Max)).is_improvement(10.0));
    }
    #[test]
    fn is_improvement_min_direction() {
        assert!(ExperimentState::new(cfg(MetricDirection::Min)).is_improvement(5.0));
    }
    #[test]
    fn is_improvement_first_run_always_true() {
        let s = ExperimentState::new(cfg(MetricDirection::Max));
        assert!(s.is_improvement(10.0));
        let mut s2 = s.clone(); s2.best_metric = Some(100.0);
        assert!(!s2.is_improvement(10.0));
    }
    #[test]
    fn record_run_updates_best() {
        let mut s = ExperimentState::new(cfg(MetricDirection::Max));
        s.record_run(ExperimentRun { run_id: 1, task_id: "".into(), agent: "".into(),
            metric_value: Some(42.0), checks_passed: Some(true), kept: true,
            timestamp: "".into(), duration_ms: None });
        assert_eq!(s.best_metric, Some(42.0));
        assert_eq!(s.best_run_id, Some(1));
    }
}