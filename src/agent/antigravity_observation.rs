// Maps agy's selected backend label to the CLI model ID.
// Used by shared completion attribution and quota marking for older unpinned tasks.
use crate::types::TaskId;

pub(crate) fn observed_model(task: &TaskId) -> Option<String> {
    let log = std::fs::read_to_string(crate::support::paths::agent_log_path(task.as_str())).ok()?;
    let label = selected_label(&log)?;
    let models =
        super::model_validation::load_from_disk_cache(crate::types::AgentKind::Antigravity)?;
    model_for_label(&models, label)
}

fn normalized_label(label: &str) -> String {
    label
        .to_ascii_lowercase()
        .chars()
        .filter(|ch| ch.is_ascii_alphanumeric())
        .collect()
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

fn model_for_label(models: &[String], label: &str) -> Option<String> {
    let id = normalized_label(label);
    models.iter().find(|model| normalized_label(model) == id).cloned()
}

#[cfg(test)]
#[path = "antigravity_observation_tests.rs"]
mod tests;
