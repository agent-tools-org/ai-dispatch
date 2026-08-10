// MiMo Code delegate spec for the OpenCode-compatible overlay engine.
// Exports agent() for get_agent wiring.
// Depends on opencode_overlay and AgentKind.

use super::opencode_overlay::{OpenCodeOverlayAgent, OpenCodeOverlaySpec};
use crate::types::AgentKind;

pub(crate) fn agent() -> OpenCodeOverlayAgent {
    OpenCodeOverlayAgent::from_spec(spec())
}

pub(crate) fn spec() -> OpenCodeOverlaySpec {
    OpenCodeOverlaySpec {
        id: "mimocode".to_string(),
        display_name: "MiMo Code".to_string(),
        reported_kind: AgentKind::MiMoCode,
        binary: "mimo".to_string(),
        extra_args: vec!["--dangerously-skip-permissions".to_string()],
        default_model: Some("mimo/mimo-auto".to_string()),
        interactive_input: true,
        rate_limit_kind: AgentKind::MiMoCode,
        allow_external_directories: false,
    }
}

#[cfg(test)]
mod tests;
