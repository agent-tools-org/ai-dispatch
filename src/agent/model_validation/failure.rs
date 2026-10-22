// Diagnostics for unavailable agent models.
// Uses model provenance and shared agent identity.

use super::*;

pub(super) fn unavailable_model(
    kind: AgentKind,
    model_clean: &str,
    source: ModelSource,
    fresh_served_list: &[String],
) -> Result<bool> {
    let list_str = fresh_served_list.join(", ");
    if source == ModelSource::Advised {
        return Err(anyhow!(
            "advised model '{model_clean}' unavailable for {}; refusing a different default. Served \
            models: {list_str}",
            kind.as_str()
        ));
    }
    if source == ModelSource::UserSupplied {
        return Err(anyhow!(
            "Agent '{}' does not serve model '{model_clean}'. Served models: {list_str}",
            kind.as_str()
        ));
    }
    aid_warn!(
        "[aid] Agent '{}' does not serve aid-selected model '{model_clean}'; dropping it and using \
            the agent's own default model",
        kind.as_str()
    );
    Ok(false)
}
