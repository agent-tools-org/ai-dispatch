// CLI handler for `aid finding` — post and list workgroup findings.
// Exports: add, list.
// Deps: store::Store.

use anyhow::Result;
use chrono::{DateTime, Local};

use crate::store::Store;

pub fn add(store: &Store, group_id: &str, content: &str, task_id: Option<&str>) -> Result<()> {
    store
        .get_workgroup(group_id)?
        .ok_or_else(|| anyhow::anyhow!("Workgroup '{group_id}' not found"))?;
    store.insert_finding(group_id, content, task_id)?;
    println!("Finding posted to {group_id}");
    Ok(())
}

pub fn list(store: &Store, group_id: &str) -> Result<()> {
    let findings = store.list_findings(group_id)?;
    if findings.is_empty() {
        println!("No findings for {group_id}.");
        return Ok(());
    }
    println!("Findings for {group_id}:");
    for finding in &findings {
        let source = finding.source_task_id.as_deref().unwrap_or("manual");
        let age = format_age(&finding.created_at);
        println!("  [{}] ({}) {}", source, age, finding.content);
    }
    Ok(())
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
