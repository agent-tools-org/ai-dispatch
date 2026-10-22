// Read-only listing of grouped unclassified provider failure markers.
// Loads marker metadata only and redacts excerpts before text or JSON display.
use crate::store::Store;
use anyhow::{Context, Result, bail};
use chrono::{DateTime, Local};
use serde::Serialize;
use std::collections::BTreeMap;

#[derive(Debug, Serialize)]
struct FailureGroup {
    time: DateTime<Local>,
    task_id: String,
    agents: BTreeMap<String, usize>,
    count: usize,
    provider_excerpt: String,
}

pub(crate) fn run(
    store: &Store,
    agent: Option<&str>,
    since: &str,
    limit: usize,
    json: bool,
) -> Result<()> {
    let (output, skipped) = report(store, agent, since, limit, json)?;
    if skipped > 0 {
        eprintln!("Skipped {skipped} malformed unclassified failure marker rows.");
    }
    println!("{output}");
    Ok(())
}

fn report(
    store: &Store,
    agent: Option<&str>,
    since: &str,
    limit: usize,
    json: bool,
) -> Result<(String, usize)> {
    let cutoff = Local::now()
        .checked_sub_signed(duration(since)?)
        .context("Duration is out of range")?;
    let (groups, skipped) = load_report(store, agent, cutoff, limit)?;
    if json {
        return Ok((serde_json::to_string_pretty(&groups)?, skipped));
    }
    if groups.is_empty() {
        return Ok(("No unclassified failure markers in range.".into(), skipped));
    }
    Ok((listing(&groups), skipped))
}

fn listing(groups: &[FailureGroup]) -> String {
    groups
        .iter()
        .map(|group| {
            let agents = group.agents.keys().cloned().collect::<Vec<_>>().join(", ");
            format!(
                "{} {} {} x{} {}",
                group.time.to_rfc3339(),
                group.task_id,
                agents,
                group.count,
                group.provider_excerpt.lines().next().unwrap_or("")
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn duration(value: &str) -> Result<chrono::Duration> {
    let split = value.char_indices().last().map_or(0, |(index, _)| index);
    let (number, unit) = value.split_at(split);
    let multiplier = match unit {
        "s" => 1,
        "m" => 60,
        "h" => 3600,
        "d" => 86400,
        "w" => 604800,
        _ => bail!("Use a duration such as 30m, 24h or 7d"),
    };
    let seconds = number
        .parse::<i64>()
        .context("Use a numeric duration such as 30m, 24h or 7d")?
        .checked_mul(multiplier)
        .filter(|n| *n >= 0)
        .context("Duration is out of range")?;
    chrono::Duration::try_seconds(seconds).context("Duration is out of range")
}

fn load_report(
    store: &Store,
    agent: Option<&str>,
    cutoff: DateTime<Local>,
    limit: usize,
) -> Result<(Vec<FailureGroup>, usize)> {
    let (mut markers, skipped) = markers(store, agent, cutoff)?;
    markers.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    let mut groups: Vec<FailureGroup> = Vec::new();
    let mut indices = BTreeMap::new();
    for (time, task_id, agent, raw) in markers.into_iter().take(limit) {
        let index = *indices.entry(raw.clone()).or_insert(groups.len());
        if index == groups.len() {
            groups.push(FailureGroup {
                time,
                task_id,
                agents: BTreeMap::new(),
                count: 0,
                provider_excerpt: redact(&raw),
            });
        }
        groups[index].count += 1;
        *groups[index].agents.entry(redact(&agent)).or_default() += 1;
    }
    Ok((groups, skipped))
}

#[path = "agent_failure_markers.rs"]
mod marker_rows;
use marker_rows::markers;

#[path = "agent_failure_redaction.rs"]
mod redaction;
use redaction::redact;

#[cfg(test)]
#[path = "agent_failures_tests.rs"]
mod tests;
