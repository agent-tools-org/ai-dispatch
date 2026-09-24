// Resolves an agent's default model and where it came from.
// Exports: resolve_default_model (sticky > forced > cli_config > catalog).
// Deps: agent_config (sticky), custom agent config, codex/qwen CLI config, catalog rows.

use crate::agent::custom::CustomAgentConfig;
use crate::types::AgentKind;

/// Catalog default: the rated row marked "default", else the first rated row.
/// Served-only rows are never a catalog default.
fn catalog_default_model(kind: AgentKind) -> Option<String> {
    let models: Vec<_> = super::models_for_agent(&kind)
        .into_iter()
        .filter(|model| model.origin == super::ModelOrigin::Catalog)
        .collect();
    models
        .iter()
        .find(|model| model.description.to_ascii_lowercase().contains("default"))
        .or_else(|| models.first())
        .map(|model| model.model.to_string())
}

/// The agent CLI's own configured default, where aid can read it.
fn cli_configured_default(kind: AgentKind) -> Option<String> {
    match kind {
        AgentKind::Codex => crate::agent::codex::cli_config::configured_model(),
        AgentKind::Qwen => super::get_qwen_selected_model(),
        _ => None,
    }
}

/// Default model and its source: sticky `aid agent config --model`, a custom
/// agent's forced model, the CLI's own config, then the catalog default.
pub(crate) fn resolve_default_model(
    name: &str,
    kind: AgentKind,
    custom_config: Option<&CustomAgentConfig>,
) -> (Option<String>, Option<&'static str>) {
    let tagged = |source: &'static str| move |model: String| (model, source);
    let resolved = crate::agent_config::get_default_model(name)
        .map(tagged("sticky"))
        .or_else(|| custom_config.and_then(|c| c.forced_model.clone()).map(tagged("forced")))
        .or_else(|| {
            if custom_config.is_some() {
                return None;
            }
            cli_configured_default(kind)
                .map(tagged("cli_config"))
                .or_else(|| catalog_default_model(kind).map(tagged("catalog")))
        });
    match resolved {
        Some((model, source)) => (Some(model), Some(source)),
        None => (None, None),
    }
}
