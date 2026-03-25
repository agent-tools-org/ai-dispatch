// CLI handler for `aid finding` — post, list, get, and review workgroup findings.
// Exports: add, list, get, update.
// Deps: store::Store, cmd::finding_render, serde_json.

use anyhow::{Result, anyhow, bail};

#[path = "finding_render.rs"]
mod finding_render;

use crate::store::Store;
use finding_render::{
    render_finding_human_output, render_finding_json_output, render_findings_human_output,
    render_findings_json_output,
};

const FINDING_VERDICTS: [&str; 4] = ["CONFIRMED", "REJECTED", "DUPLICATE", "INVALID"];

#[allow(clippy::too_many_arguments)]
pub fn add(
    store: &Store,
    group_id: &str,
    content: &str,
    task_id: Option<&str>,
    severity: Option<&str>,
    title: Option<&str>,
    finding_file: Option<&str>,
    lines: Option<&str>,
    category: Option<&str>,
    confidence: Option<&str>,
) -> Result<()> {
    ensure_workgroup_exists(store, group_id)?;
    store.insert_finding(
        group_id,
        content,
        task_id,
        severity,
        title,
        finding_file,
        lines,
        category,
        confidence,
    )?;
    println!("Finding posted to {group_id}");
    Ok(())
}

pub fn list(
    store: &Store,
    group_id: &str,
    json: bool,
    count: bool,
    severity: Option<&str>,
    verdict: Option<&str>,
) -> Result<()> {
    let verdict = normalize_optional_verdict(verdict)?;
    let findings = store.list_findings_filtered(group_id, severity, verdict.as_deref())?;
    if count {
        println!("{}", findings.len());
        return Ok(());
    }
    if json {
        println!("{}", render_findings_json_output(group_id, &findings)?);
        return Ok(());
    }
    println!("{}", render_findings_human_output(group_id, &findings));
    Ok(())
}

pub fn get(store: &Store, group_id: &str, finding_id: i64, json: bool) -> Result<()> {
    ensure_workgroup_exists(store, group_id)?;
    let finding = store
        .get_finding(group_id, finding_id)?
        .ok_or_else(|| anyhow!("Finding '{finding_id}' not found in workgroup '{group_id}'"))?;
    if json {
        println!("{}", render_finding_json_output(group_id, &finding)?);
    } else {
        println!("{}", render_finding_human_output(group_id, &finding));
    }
    Ok(())
}

pub fn update(
    store: &Store,
    group_id: &str,
    finding_id: i64,
    verdict: Option<&str>,
    score: Option<&str>,
    note: Option<&str>,
) -> Result<()> {
    ensure_workgroup_exists(store, group_id)?;
    let verdict = normalize_optional_verdict(verdict)?;
    let score = normalize_score_json(score)?;
    store.update_finding(
        group_id,
        finding_id,
        verdict.as_deref(),
        score.as_deref(),
        note,
    )?;
    println!("Finding {finding_id} updated in {group_id}");
    Ok(())
}

fn ensure_workgroup_exists(store: &Store, group_id: &str) -> Result<()> {
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow!("Workgroup '{group_id}' not found"))?;
    Ok(())
}

fn normalize_optional_verdict(value: Option<&str>) -> Result<Option<String>> {
    value.map(normalize_verdict).transpose()
}

fn normalize_verdict(value: &str) -> Result<String> {
    let normalized = value.trim().to_ascii_uppercase();
    if FINDING_VERDICTS.contains(&normalized.as_str()) {
        Ok(normalized)
    } else {
        bail!(
            "Invalid verdict '{value}'. Expected one of: {}",
            FINDING_VERDICTS.join(", ")
        )
    }
}

fn normalize_score_json(value: Option<&str>) -> Result<Option<String>> {
    value.map(parse_score_json).transpose()
}

fn parse_score_json(value: &str) -> Result<String> {
    serde_json::from_str::<serde_json::Value>(value)
        .map(|parsed| parsed.to_string())
        .map_err(|err| anyhow!("Invalid score JSON: {err}"))
}

#[cfg(test)]
mod tests {
    use super::{normalize_optional_verdict, parse_score_json};

    #[test]
    fn normalize_optional_verdict_accepts_known_values() {
        let verdict = normalize_optional_verdict(Some("confirmed")).unwrap();

        assert_eq!(verdict.as_deref(), Some("CONFIRMED"));
    }

    #[test]
    fn normalize_optional_verdict_rejects_unknown_values() {
        let error = normalize_optional_verdict(Some("maybe")).unwrap_err();

        assert!(error.to_string().contains("Invalid verdict"));
    }

    #[test]
    fn parse_score_json_normalizes_valid_payloads() {
        let score = parse_score_json(r#"{ "confidence": "high", "impact": 9 }"#).unwrap();

        assert_eq!(score, r#"{"confidence":"high","impact":9}"#);
    }
}
