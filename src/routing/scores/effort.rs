// Read explicit CLI effort configuration without guessing a harness default.
// Exports: configured; absent or unsupported effort stays unknown.
// Deps: the existing Codex home resolver, operator home, TOML and JSON.

use crate::types::AgentKind;

pub(super) fn configured(agent: AgentKind) -> Option<String> {
    match agent {
        AgentKind::Codex => {
            let path = crate::agent::codex::cli_config::codex_home()
                .ok()?
                .join("config.toml");
            let text = std::fs::read_to_string(path).ok()?;
            let config: toml::Table = toml::from_str(&text).ok()?;
            config
                .get("model_reasoning_effort")?
                .as_str()
                .map(str::to_string)
        }
        AgentKind::Claude => {
            if let Ok(effort) = std::env::var("CLAUDE_CODE_EFFORT_LEVEL") {
                return (!effort.trim().is_empty()).then_some(effort);
            }
            let path = crate::agent::home_isolation::resolve_real_home()
                .ok()?
                .join(".claude/settings.json");
            let config: serde_json::Value =
                serde_json::from_slice(&std::fs::read(path).ok()?).ok()?;
            config.get("effortLevel")?.as_str().map(str::to_string)
        }
        _ => None,
    }
}
