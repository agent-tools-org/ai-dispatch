// Checklist completion display for `aid show` — parses <aid-checklist> from prompts
// and matches CONFIRMED/REJECTED from task output via checklist_scan.

use crate::cmd::checklist_scan::{scan_checklist, ChecklistItemStatus};
use crate::cmd::show::output_text_for_task;
use crate::store::Store;
use crate::types::Task;

pub(crate) fn extract_checklist_from_prompt(prompt: &str) -> Vec<String> {
    let Some(start) = prompt.find("<aid-checklist>") else {
        return Vec::new();
    };
    let Some(end) = prompt.find("</aid-checklist>") else {
        return Vec::new();
    };
    let block_start = start + "<aid-checklist>".len();
    if block_start > end {
        return Vec::new();
    }
    let block = &prompt[block_start..end];
    let mut items = Vec::new();
    for line in block.lines() {
        let line = line.trim();
        let Some(rest) = line.strip_prefix("[ ] ") else {
            continue;
        };
        let Some((num, body)) = rest.split_once('.') else {
            continue;
        };
        if num.trim().parse::<usize>().is_err() {
            continue;
        }
        let body = body.trim();
        if !body.is_empty() {
            items.push(body.to_string());
        }
    }
    items
}

pub(crate) fn render_checklist_status(store: &Store, task: &Task) -> Option<String> {
    let items = extract_checklist_from_prompt(&task.prompt);
    if items.is_empty() {
        return None;
    }
    let output = output_text_for_task(store, task.id.as_str(), true).unwrap_or_default();
    let scanned = scan_checklist(&items, &output);
    let total = scanned.items.len();
    let addressed = scanned
        .items
        .iter()
        .filter(|i| {
            matches!(
                i.status,
                ChecklistItemStatus::Confirmed | ChecklistItemStatus::Rejected
            )
        })
        .count();
    let mut out = format!("Checklist: {addressed}/{total} addressed\n");
    for (idx, it) in scanned.items.iter().enumerate() {
        let mark = match it.status {
            ChecklistItemStatus::Confirmed | ChecklistItemStatus::Rejected => "✓",
            ChecklistItemStatus::Missing => "✗",
        };
        let label = match it.status {
            ChecklistItemStatus::Confirmed => "CONFIRMED",
            ChecklistItemStatus::Rejected => "REJECTED",
            ChecklistItemStatus::Missing => "MISSING",
        };
        out.push_str(&format!(
            " {} {}. {} — {}\n",
            mark, idx + 1, it.item, label
        ));
    }
    Some(out)
}
