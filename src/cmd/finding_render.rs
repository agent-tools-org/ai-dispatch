// Rendering helpers for `aid finding` human and JSON output.
// Exports: list/get render functions consumed by cmd::finding.
// Deps: crate::types::Finding, chrono, serde_json.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde_json::{Value, json};

use crate::types::Finding;

pub(super) fn render_findings_json_output(group_id: &str, findings: &[Finding]) -> Result<String> {
    let json_findings = findings
        .iter()
        .map(|finding| finding_json_value(group_id, finding))
        .collect::<Result<Vec<_>>>()?;
    Ok(serde_json::to_string_pretty(&json_findings)?)
}

pub(super) fn render_finding_json_output(group_id: &str, finding: &Finding) -> Result<String> {
    Ok(serde_json::to_string_pretty(&finding_json_value(group_id, finding)?)?)
}

pub(super) fn render_findings_human_output(group_id: &str, findings: &[Finding]) -> String {
    if findings.is_empty() {
        return format!("No findings for {group_id}.");
    }

    let mut output = format!("Findings for {group_id}:");
    for finding in findings {
        append_list_entry(&mut output, finding);
    }
    output
}

pub(super) fn render_finding_human_output(group_id: &str, finding: &Finding) -> String {
    let mut output = format!("Finding {} for {group_id}:", finding.id);
    output.push_str(&format!("\n  Source: {}", finding.source_task_id.as_deref().unwrap_or("manual")));
    output.push_str(&format!("\n  Created: {}", finding.created_at.format("%Y-%m-%d %H:%M:%S")));
    append_optional_detail(&mut output, "Updated", finding.updated_at.as_ref().map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string()).as_deref());
    append_optional_detail(&mut output, "Severity", finding.severity.as_deref());
    append_optional_detail(&mut output, "Verdict", finding.verdict.as_deref());
    append_optional_detail(&mut output, "Title", finding.title.as_deref());
    append_optional_detail(&mut output, "Category", finding.category.as_deref());
    append_optional_detail(&mut output, "Confidence", finding.confidence.as_deref());
    append_file_line(&mut output, finding);
    append_score_line(&mut output, finding.score.as_deref());
    append_optional_detail(&mut output, "Note", finding.note.as_deref());
    output.push_str(&format!("\n  Content:\n    {}", finding.content));
    output
}

fn finding_json_value(group_id: &str, finding: &Finding) -> Result<Value> {
    Ok(json!({
        "id": finding.id,
        "content": finding.content,
        "source_task_id": finding.source_task_id,
        "group_id": group_id,
        "severity": finding.severity,
        "title": finding.title,
        "file": finding.file,
        "lines": finding.lines,
        "category": finding.category,
        "confidence": finding.confidence,
        "verdict": finding.verdict,
        "score": score_json_value(finding.score.as_deref())?,
        "note": finding.note,
        "created_at": finding.created_at.to_rfc3339(),
        "updated_at": finding.updated_at.as_ref().map(|dt| dt.to_rfc3339()),
    }))
}

fn score_json_value(score: Option<&str>) -> Result<Value> {
    match score {
        Some(value) => match serde_json::from_str(value) {
            Ok(parsed) => Ok(parsed),
            Err(_) => Ok(Value::String(value.to_string())),
        },
        None => Ok(Value::Null),
    }
}

fn append_list_entry(output: &mut String, finding: &Finding) {
    let source = finding.source_task_id.as_deref().unwrap_or("manual");
    let age = format_age(&finding.created_at);
    let severity_tag = finding
        .severity
        .as_deref()
        .map(|value| format!(" [{value}]"))
        .unwrap_or_default();
    let verdict_tag = finding
        .verdict
        .as_deref()
        .map(|value| format!(" <{value}>"))
        .unwrap_or_default();
    let title_part = finding
        .title
        .as_deref()
        .map(|value| format!(" {value}"))
        .unwrap_or_default();
    let category_part = finding
        .category
        .as_deref()
        .map(|value| format!(" ({value})"))
        .unwrap_or_default();
    output.push_str(&format!(
        "\n  #{} [{}] ({}){}{}{}{}",
        finding.id, source, age, severity_tag, verdict_tag, title_part, category_part
    ));
    output.push_str(&format!("\n    {}", finding.content));
    append_file_line(output, finding);
    append_score_line(output, finding.score.as_deref());
    append_optional_detail(output, "Note", finding.note.as_deref());
}

fn append_file_line(output: &mut String, finding: &Finding) {
    if let (Some(file), Some(lines)) = (finding.file.as_deref(), finding.lines.as_deref()) {
        output.push_str(&format!("\n    File: {file}:{lines}"));
    } else if let Some(file) = finding.file.as_deref() {
        output.push_str(&format!("\n    File: {file}"));
    }
}

fn append_score_line(output: &mut String, score: Option<&str>) {
    if let Some(score) = score {
        output.push_str(&format!("\n    Score: {score}"));
    }
}

fn append_optional_detail(output: &mut String, label: &str, value: Option<&str>) {
    if let Some(value) = value {
        output.push_str(&format!("\n    {label}: {value}"));
    }
}

fn format_age(created_at: &DateTime<Local>) -> String {
    let duration = Local::now().signed_duration_since(*created_at);
    let mins = duration.num_minutes();
    if mins <= 0 {
        format!("{}s", duration.num_seconds().max(0))
    } else if mins < 60 {
        format!("{}m", mins)
    } else if mins < 24 * 60 {
        format!("{}h", duration.num_hours())
    } else {
        format!("{}d", duration.num_days())
    }
}

#[cfg(test)]
mod tests {
    use super::{
        render_finding_human_output, render_finding_json_output, render_findings_human_output,
        render_findings_json_output,
    };
    use crate::store::Store;
    use serde_json::Value;

    fn test_store() -> Store {
        let store = Store::open_memory().unwrap();
        store
            .create_workgroup("dispatch", "Shared repo rules.", None, Some("wg-abc"))
            .unwrap();
        store
    }

    #[test]
    fn human_output_for_empty_findings_is_unchanged() {
        let output = render_findings_human_output("wg-abc", &[]);

        assert_eq!(output, "No findings for wg-abc.");
    }

    #[test]
    fn human_output_lists_findings_in_existing_format() {
        let store = test_store();
        store
            .insert_finding("wg-abc", "first finding", None, None, None, None, None, None, None)
            .unwrap();

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_findings_human_output("wg-abc", &findings);

        assert!(output.starts_with("Findings for wg-abc:\n  #1 [manual] ("));
        assert!(output.contains("first finding"));
    }

    #[test]
    fn json_output_includes_review_fields() {
        let store = test_store();
        store
            .insert_finding(
                "wg-abc",
                "body",
                Some("t-1"),
                Some("MEDIUM"),
                Some("A title"),
                Some("foo.rs"),
                Some("10-20"),
                Some("reentrancy"),
                Some("low"),
            )
            .unwrap();
        store
            .update_finding(
                "wg-abc",
                1,
                Some("CONFIRMED"),
                Some(r#"{"confidence":"high","impact":9}"#),
                Some("validated"),
            )
            .unwrap();

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_findings_json_output("wg-abc", &findings).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();
        let obj = &parsed.as_array().unwrap()[0];

        assert_eq!(obj["severity"], "MEDIUM");
        assert_eq!(obj["verdict"], "CONFIRMED");
        assert_eq!(obj["score"]["confidence"], "high");
        assert_eq!(obj["score"]["impact"], 9);
        assert_eq!(obj["note"], "validated");
        assert!(obj["updated_at"].as_str().is_some());
    }

    #[test]
    fn single_finding_json_output_includes_review_fields() {
        let store = test_store();
        store
            .insert_finding("wg-abc", "body", None, None, None, None, None, None, None)
            .unwrap();
        store
            .update_finding(
                "wg-abc",
                1,
                Some("REJECTED"),
                Some(r#"{"confidence":"low"}"#),
                Some("duplicate with finding 7"),
            )
            .unwrap();

        let finding = store.get_finding("wg-abc", 1).unwrap().unwrap();
        let output = render_finding_json_output("wg-abc", &finding).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed["id"], 1);
        assert_eq!(parsed["verdict"], "REJECTED");
        assert_eq!(parsed["score"]["confidence"], "low");
        assert_eq!(parsed["note"], "duplicate with finding 7");
    }

    #[test]
    fn detailed_human_output_shows_review_metadata() {
        let store = test_store();
        store
            .insert_finding(
                "wg-abc",
                "swap() accepts amountOutMin=0",
                Some("t-1234"),
                Some("HIGH"),
                Some("Missing slippage protection"),
                Some("src/Router.sol"),
                Some("145-160"),
                Some("MEV"),
                Some("high"),
            )
            .unwrap();
        store
            .update_finding(
                "wg-abc",
                1,
                Some("CONFIRMED"),
                Some(r#"{"confidence":"high"}"#),
                Some("validated against production call path"),
            )
            .unwrap();

        let finding = store.get_finding("wg-abc", 1).unwrap().unwrap();
        let output = render_finding_human_output("wg-abc", &finding);

        assert!(output.contains("Finding 1 for wg-abc:"));
        assert!(output.contains("Verdict: CONFIRMED"));
        assert!(output.contains(r#"Score: {"confidence":"high"}"#));
        assert!(output.contains("Note: validated against production call path"));
    }
}
