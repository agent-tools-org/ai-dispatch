// Display experiment status and run history.
// Exports: run_status().
// Deps: experiment_types, experiment_persist.

use anyhow::Result;

pub fn run_status(dir: Option<&str>) -> Result<()> {
    let dir = dir.unwrap_or(".");
    let path = super::experiment_persist::experiment_file_path(dir);
    if !path.exists() {
        eprintln!("[aid] No experiment.jsonl found in {dir}");
        return Ok(());
    }
    let dummy = super::experiment_types::ExperimentConfig {
        metric_command: String::new(),
        direction: super::experiment_types::MetricDirection::Max,
        agent: String::new(),
        prompt: String::new(),
        checks: None,
        max_runs: None,
        worktree: None,
    };
    let state = super::experiment_persist::load_state(&path, &dummy)?;
    if state.runs.is_empty() {
        eprintln!("[aid] No experiment runs recorded yet");
        return Ok(());
    }
    println!(
        "Experiment: {} runs | Best: {} (run #{})",
        state.runs.len(),
        state.best_metric.map(|v| format!("{v:.4}")).unwrap_or("n/a".into()),
        state.best_run_id.unwrap_or(0),
    );
    println!(
        "{:<6} {:<10} {:<12} {:<8} {:<8} {}",
        "Run", "Agent", "Metric", "Kept", "Checks", "Duration"
    );
    println!("{}", "-".repeat(60));
    for run in &state.runs {
        let metric = run.metric_value.map(|v| format!("{v:.4}")).unwrap_or("err".into());
        let kept = if run.kept { "yes" } else { "no" };
        let checks = match run.checks_passed {
            Some(true) => "pass",
            Some(false) => "fail",
            None => "-",
        };
        let duration = run.duration_ms.map(|d| format!("{}s", d / 1000)).unwrap_or("-".into());
        println!(
            "{:<6} {:<10} {:<12} {:<8} {:<8} {}",
            run.run_id, run.agent, metric, kept, checks, duration
        );
    }
    Ok(())
}
