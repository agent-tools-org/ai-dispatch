// OpenCode `models` CLI probe and parser.
// Exports: probe_served_models, parse_opencode_models_output.
// Deps: model_validation::run_probe_cmd.

use anyhow::Result;
use std::process::Command;

pub(crate) fn probe_served_models() -> Result<Option<Vec<String>>> {
    let mut cmd = Command::new("opencode");
    cmd.arg("models");
    let Some(output) = super::model_validation::run_probe_cmd(cmd) else {
        return Ok(None);
    };
    let models = parse_opencode_models_output(&output.stdout);
    Ok(if models.is_empty() { None } else { Some(models) })
}

pub(crate) fn parse_opencode_models_output(output: &str) -> Vec<String> {
    let mut models = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty()
            || trimmed.starts_with('#')
            || trimmed.starts_with("Fetching")
            || trimmed.starts_with("Available")
            || trimmed.starts_with("ERROR")
            || trimmed.starts_with("error")
            || trimmed.starts_with("[ERROR]")
            || trimmed.starts_with("[WARN]")
        {
            continue;
        }
        let first = trimmed.split_whitespace().next().unwrap_or("");
        let clean = first.trim_matches(|c: char| {
            !c.is_alphanumeric() && !matches!(c, '-' | '.' | '/' | ':' | '_')
        });
        if !clean.is_empty() && clean.contains('/') && !models.iter().any(|m| m == clean) {
            models.push(clean.to_string());
        }
    }
    models
}

#[cfg(test)]
#[path = "opencode_model_tests.rs"]
mod tests;
