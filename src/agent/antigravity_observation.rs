// Maps agy's selected backend label to the CLI model ID.
// Used by shared completion attribution and quota marking for older unpinned tasks.
use crate::types::TaskId;

pub(crate) fn observed_model(task: &TaskId) -> Option<String> {
    let log = std::fs::read_to_string(crate::support::paths::agent_log_path(task.as_str())).ok()?;
    let label = selected_label(&log)?;
    let output = super::model_validation::run_probe_cmd("agy", &["models"])?;
    model_for_label(&output.stdout, label)
}

fn selected_label(log: &str) -> Option<&str> {
    log.lines().rev().find_map(|line| {
        let rest = line
            .split_once("Propagating selected model override to backend:")?
            .1;
        rest.split_once("label=\"")?
            .1
            .split_once('"')
            .map(|(label, _)| label)
    })
}

fn model_for_label(models: &str, label: &str) -> Option<String> {
    models.lines().find_map(|line| {
        let line = line.trim();
        let end = line.find(char::is_whitespace)?;
        (line[end..].trim() == label).then(|| line[..end].to_string())
    })
}

#[cfg(test)]
#[path = "antigravity_observation_tests.rs"]
mod tests;
