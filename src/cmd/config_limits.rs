// Clear default, registered custom and Codex provider quota markers.
// Keeps provider names validated before using them as marker slugs.
use super::*;

pub(super) fn clear_limit(agent: &str) -> Result<()> {
    crate::agent::removed::reject(agent)?;
    if agent == "all" {
        clear_registered_markers();
        clear_provider_markers()?;
        return Ok(());
    }
    if let Some(kind) = AgentKind::parse_str(agent) {
        if rate_limit::clear_all_rate_limits_for_agent(&kind, None) {
            println!("Cleared rate-limit for {}", kind.as_str());
        } else {
            println!("{} is not rate-limited", agent);
        }
        return Ok(());
    }
    if let Some(id) = crate::agent::codex::provider::route_id(agent) {
        anyhow::ensure!(
            !id.is_empty()
                && id
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_'),
            "Codex provider id must contain only letters, digits, hyphens or underscores"
        );
        rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Codex, Some(agent));
        println!("Cleared rate-limit for {agent}");
        return Ok(());
    }
    if crate::agent::registry::custom_agent_exists(agent) {
        if rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Custom, Some(agent)) {
            println!("Cleared rate-limit for {agent}");
        } else {
            println!("{agent} is not rate-limited");
        }
        return Ok(());
    }
    anyhow::bail!("Unknown agent: {agent}");
}

fn clear_registered_markers() {
    for (kind, _, _, _, _) in AGENT_PROFILES {
        if rate_limit::clear_all_rate_limits_for_agent(kind, None) {
            println!("Cleared rate-limit for {}", kind.as_str());
        }
    }
    for config in crate::agent::registry::list_custom_agents() {
        if rate_limit::clear_all_rate_limits_for_agent(&AgentKind::Custom, Some(config.id.as_str()))
        {
            println!("Cleared rate-limit for {}", config.id);
        }
    }
}

fn clear_provider_markers() -> Result<()> {
    let dir = crate::support::paths::aid_dir();
    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(error) => return Err(error.into()),
    };
    for entry in entries {
        let entry = entry?;
        if entry
            .file_name()
            .to_string_lossy()
            .strip_prefix("rate-limit-")
            .and_then(crate::agent::codex::provider::route_id)
            .is_some()
        {
            fs::remove_file(entry.path())?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "config_provider_tests.rs"]
mod tests;
