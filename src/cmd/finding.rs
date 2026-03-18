// CLI handler for `aid finding` — post and list workgroup findings.
// Exports: add, list.
// Deps: store::Store.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde_json;

use crate::types::Finding;
use crate::store::Store;

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
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{group_id}' not found"))?;
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

pub fn list(store: &Store, group_id: &str, json: bool, count: bool) -> Result<()> {
    let findings = store.list_findings(group_id)?;
    if count {
        println!("{}", findings.len());
        return Ok(());
    }
    if json {
        println!("{}", render_json_output(group_id, &findings)?);
        return Ok(());
    }
    println!("{}", render_human_output(group_id, &findings));
    Ok(())
}

fn render_json_output(group_id: &str, findings: &[Finding]) -> Result<String> {
    let json_findings: Vec<serde_json::Value> = findings
        .iter()
        .map(|finding| {
            serde_json::json!({
                "content": finding.content,
                "source_task_id": finding.source_task_id,
                "group_id": group_id,
                "severity": finding.severity,
                "title": finding.title,
                "file": finding.file,
                "lines": finding.lines,
                "category": finding.category,
                "confidence": finding.confidence,
                "created_at": finding.created_at.to_rfc3339(),
            })
        })
        .collect();
    Ok(serde_json::to_string_pretty(&json_findings)?)
}

fn render_human_output(group_id: &str, findings: &[Finding]) -> String {
    if findings.is_empty() {
        return format!("No findings for {group_id}.");
    }

    let mut output = format!("Findings for {group_id}:");
    for finding in findings {
        let source = finding.source_task_id.as_deref().unwrap_or("manual");
        let age = format_age(&finding.created_at);
        let severity_tag = finding
            .severity
            .as_deref()
            .map(|s| format!(" [{}]", s))
            .unwrap_or_default();
        let title_part = finding
            .title
            .as_deref()
            .map(|t| format!(" {} ", t))
            .unwrap_or_default();
        let category_part = finding
            .category
            .as_deref()
            .map(|c| format!(" ({})", c))
            .unwrap_or_default();
        output.push_str(&format!(
            "\n  [{}] ({}){}{}{}",
            source, age, severity_tag, title_part, category_part
        ));
        output.push_str(&format!("\n    {}", finding.content));
        if let (Some(f), Some(l)) = (finding.file.as_deref(), finding.lines.as_deref()) {
            output.push_str(&format!("\n    File: {}:{}", f, l));
        } else if let Some(f) = finding.file.as_deref() {
            output.push_str(&format!("\n    File: {}", f));
        }
    }
    output
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
    use super::{render_human_output, render_json_output};
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
        let output = render_human_output("wg-abc", &[]);

        assert_eq!(output, "No findings for wg-abc.");
    }

    #[test]
    fn json_output_is_valid_json() {
        let store = test_store();
        store
            .insert_finding(
                "wg-abc",
                "first finding",
                Some("t-1234"),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_json_output("wg-abc", &findings).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        assert_eq!(parsed.as_array().unwrap().len(), 1);
        assert_eq!(parsed[0]["content"], "first finding");
        assert_eq!(parsed[0]["source_task_id"], "t-1234");
        assert_eq!(parsed[0]["group_id"], "wg-abc");
        assert!(parsed[0]["created_at"].as_str().is_some());
    }

    #[test]
    fn human_output_lists_findings_in_existing_format() {
        let store = test_store();
        store
            .insert_finding("wg-abc", "first finding", None, None, None, None, None, None, None)
            .unwrap();

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_human_output("wg-abc", &findings);

        assert!(output.starts_with("Findings for wg-abc:\n  [manual] ("));
        assert!(output.contains("first finding"));
    }

    #[test]
    fn structured_finding_renders_severity_and_title() {
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

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_human_output("wg-abc", &findings);

        assert!(output.contains("[HIGH]"));
        assert!(output.contains("Missing slippage protection"));
        assert!(output.contains("(MEV)"));
        assert!(output.contains("File: src/Router.sol:145-160"));
    }

    #[test]
    fn json_output_includes_structured_fields() {
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

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_json_output("wg-abc", &findings).unwrap();
        let parsed: Value = serde_json::from_str(&output).unwrap();

        let obj = &parsed.as_array().unwrap()[0];
        assert_eq!(obj["severity"], "MEDIUM");
        assert_eq!(obj["title"], "A title");
        assert_eq!(obj["file"], "foo.rs");
        assert_eq!(obj["lines"], "10-20");
        assert_eq!(obj["category"], "reentrancy");
        assert_eq!(obj["confidence"], "low");
    }

    #[test]
    fn count_output_matches_number_of_findings() {
        let store = test_store();
        store
            .insert_finding("wg-abc", "first finding", None, None, None, None, None, None, None)
            .unwrap();
        store
            .insert_finding(
                "wg-abc",
                "second finding",
                Some("t-2"),
                None,
                None,
                None,
                None,
                None,
                None,
            )
            .unwrap();

        let findings = store.list_findings("wg-abc").unwrap();

        assert_eq!(findings.len(), 2);
    }
}
