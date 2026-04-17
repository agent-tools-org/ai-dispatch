// Project profile defaults for .aid/project.toml.
// Exports apply_profile() to keep project.rs compact and focused on parsing.
// Deps: super::ProjectConfig only.

use super::ProjectConfig;

pub(super) fn apply_profile(config: &mut ProjectConfig) {
    let profile = config.profile.as_deref().map(str::to_lowercase);
    let profile = match profile {
        Some(ref value) => value.as_str(),
        None => return,
    };

    match profile {
        "hobby" => apply_hobby_profile(config),
        "standard" => apply_standard_profile(config),
        "production" => apply_production_profile(config),
        _ => {}
    }
}

fn apply_hobby_profile(config: &mut ProjectConfig) {
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(2.0);
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(5.0);
    }
    config.budget.prefer_budget = true;
}

fn apply_standard_profile(config: &mut ProjectConfig) {
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(10.0);
    }
    if config.verify.is_none() {
        config.verify = Some("auto".to_string());
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(20.0);
    }
    append_rule(
        &mut config.rules,
        "All new functions must have at least one test",
    );
    config.budget.prefer_budget = false;
}

fn apply_production_profile(config: &mut ProjectConfig) {
    if config.max_task_cost.is_none() {
        config.max_task_cost = Some(25.0);
    }
    if config.verify.is_none() {
        config.verify = Some(default_production_verify(config));
    }
    if config.budget.cost_limit_usd.is_none() {
        config.budget.cost_limit_usd = Some(50.0);
    }
    append_rule(&mut config.rules, "All changes must have tests");
    append_rule(&mut config.rules, "No unwrap() in production code");
    append_rule(&mut config.rules, "Changes require cross-review");
    config.budget.prefer_budget = false;
}

fn default_production_verify(config: &ProjectConfig) -> String {
    let language = config.language.as_deref().unwrap_or("").to_lowercase();
    if language == "typescript" || language == "javascript" || language == "node" {
        "npm test".to_string()
    } else {
        "cargo test".to_string()
    }
}

fn append_rule(rules: &mut Vec<String>, rule: &str) {
    if !rules.iter().any(|existing| existing == rule) {
        rules.push(rule.to_string());
    }
}
