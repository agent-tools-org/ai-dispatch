// CLI handler for `aid finding` — post and list workgroup findings.
// Exports: add, list.
// Deps: store::Store.

use anyhow::Result;
use chrono::{DateTime, Local};
use serde_json;

use crate::types::Finding;
use crate::store::Store;

pub fn add(store: &Store, group_id: &str, content: &str, task_id: Option<&str>) -> Result<()> {
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{group_id}' not found"))?;
    store.insert_finding(group_id, content, task_id)?;
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
        output.push_str(&format!("\n  [{}] ({}) {}", source, age, finding.content));
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
            .insert_finding("wg-abc", "first finding", Some("t-1234"))
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
        store.insert_finding("wg-abc", "first finding", None).unwrap();

        let findings = store.list_findings("wg-abc").unwrap();
        let output = render_human_output("wg-abc", &findings);

        assert!(output.starts_with("Findings for wg-abc:\n  [manual] ("));
        assert!(output.ends_with(" first finding"));
    }

    #[test]
    fn count_output_matches_number_of_findings() {
        let store = test_store();
        store.insert_finding("wg-abc", "first finding", None).unwrap();
        store.insert_finding("wg-abc", "second finding", Some("t-2")).unwrap();

        let findings = store.list_findings("wg-abc").unwrap();

        assert_eq!(findings.len(), 2);
    }
}
