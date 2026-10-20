// Reject removed execution routes while keeping stored agent names readable.
// Exports: is_removed, reject, validate_run, validate_team. Deps: anyhow, RunArgs, team config.

const REMOVED_AGENTS: &[&str] = &["gemini", "qwen"];

pub(crate) fn is_removed(name: &str) -> bool {
    REMOVED_AGENTS.iter().any(|removed| name.trim().eq_ignore_ascii_case(removed))
}

pub(crate) fn reject(name: &str) -> anyhow::Result<()> {
    if is_removed(name) {
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
    validate_team(args.team.as_deref())
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
