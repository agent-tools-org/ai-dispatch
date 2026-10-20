// Reject removed execution routes while keeping stored agent names readable.
// Exports: reject, validate_run, validate_agent_config. Deps: anyhow, RunArgs, agent config.

pub(crate) fn reject(name: &str) -> anyhow::Result<()> {
    if matches!(name.trim().to_ascii_lowercase().as_str(), "gemini" | "qwen") {
        anyhow::bail!(
            "Agent '{}' was removed in this release; use `agy`, or a Google provider through \
             `opencode`/`kilo` or a custom agent.",
            name.trim()
        );
    }
    Ok(())
}

pub(crate) fn validate_run(args: &crate::cmd::run::RunArgs) -> anyhow::Result<()> {
    for name in std::iter::once(args.agent_name.as_str())
        .chain(args.judge.as_deref())
        .chain(args.peer_review.as_deref())
        .chain(args.cascade.iter().map(String::as_str))
    {
        for name in name.split(',') {
            reject(name)?;
        }
    }
    validate_team(args.team.as_deref())?;
    validate_agent_config()
}

pub(crate) fn validate_agent_config() -> anyhow::Result<()> {
    for name in crate::config::agent::load_agent_config().keys() {
        reject(name)?;
    }
    Ok(())
}

pub(crate) fn validate_team(name: Option<&str>) -> anyhow::Result<()> {
    let Some(name) = name else { return Ok(()) };
    let dir = crate::config::team::teams_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Ok(());
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path.extension().and_then(|value| value.to_str()) != Some("toml") {
            continue;
        }
        let content = std::fs::read_to_string(&path)?;
        if let Ok(file) = toml::from_str::<crate::config::team::TeamFile>(&content)
            && (file.team.id == name
                || path.file_stem().and_then(|value| value.to_str()) == Some(name))
        {
            crate::config::team::parse_team(&content)?;
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "removed_tests.rs"]
mod tests;
