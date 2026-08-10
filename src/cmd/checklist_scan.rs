// Parses the explicit checklist response contract injected into task prompts.
// Exports checklist results used for reporting and optional retries.
// Deps: standard string processing only.

pub(crate) struct ChecklistItemResult {
    pub item: String,
    pub status: ChecklistItemStatus,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum ChecklistItemStatus {
    Confirmed,
    Rejected,
    Missing,
}

pub(crate) struct ChecklistResult {
    pub items: Vec<ChecklistItemResult>,
}

impl ChecklistResult {
    pub fn all_addressed(&self) -> bool {
        self.items
            .iter()
            .all(|item| item.status != ChecklistItemStatus::Missing)
    }

    pub fn missing_items(&self) -> Vec<&str> {
        self.items
            .iter()
            .filter(|item| item.status == ChecklistItemStatus::Missing)
            .map(|item| item.item.as_str())
            .collect()
    }

    pub fn summary(&self) -> String {
        let total = self.items.len();
        let confirmed = self
            .items
            .iter()
            .filter(|item| item.status == ChecklistItemStatus::Confirmed)
            .count();
        let rejected = self
            .items
            .iter()
            .filter(|item| item.status == ChecklistItemStatus::Rejected)
            .count();
        format!(
            "{}/{} addressed ({} confirmed, {} rejected)",
            confirmed + rejected,
            total,
            confirmed,
            rejected
        )
    }
}

pub(crate) fn scan_checklist(items: &[String], output: &str) -> ChecklistResult {
    ChecklistResult {
        items: items
            .iter()
            .enumerate()
            .map(|(index, item)| ChecklistItemResult {
                item: item.clone(),
                status: explicit_status(index + 1, output),
            })
            .collect(),
    }
}

fn explicit_status(number: usize, output: &str) -> ChecklistItemStatus {
    let prefix = format!("checklist {number}:");
    output
        .lines()
        .find_map(|line| status_after_prefix(line.trim(), &prefix))
        .unwrap_or(ChecklistItemStatus::Missing)
}

fn status_after_prefix(line: &str, prefix: &str) -> Option<ChecklistItemStatus> {
    let lower = line.to_ascii_lowercase();
    let rest = lower.strip_prefix(prefix)?.trim_start();
    let status = rest.split_whitespace().next()?.trim_end_matches([':', '-', '—']);
    match status {
        "confirmed" => Some(ChecklistItemStatus::Confirmed),
        "rejected" => Some(ChecklistItemStatus::Rejected),
        _ => None,
    }
}

#[cfg(test)]
#[path = "checklist_scan_tests.rs"]
mod tests;
