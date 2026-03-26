// Scans agent output for per-item CONFIRMED / REJECTED checklist responses.
// Exports: ChecklistResult, scan_checklist(), and related types. Deps: std only.

const NEAR: usize = 200;

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
        self.items.iter().all(|i| !matches!(i.status, ChecklistItemStatus::Missing))
    }
    pub fn missing_items(&self) -> Vec<&str> {
        self.items.iter().filter(|i| matches!(i.status, ChecklistItemStatus::Missing)).map(|i| i.item.as_str()).collect()
    }
    pub fn summary(&self) -> String {
        let total = self.items.len();
        let (c, r) = self.items.iter().fold((0usize, 0usize), |(c, r), i| match i.status {
            ChecklistItemStatus::Confirmed => (c + 1, r),
            ChecklistItemStatus::Rejected => (c, r + 1),
            ChecklistItemStatus::Missing => (c, r),
        });
        format!("{}/{} addressed ({} confirmed, {} rejected)", c + r, total, c, r)
    }
}

pub(crate) fn scan_checklist(checklist_items: &[String], output_text: &str) -> ChecklistResult {
    ChecklistResult {
        items: checklist_items
            .iter()
            .enumerate()
            .map(|(idx, item)| ChecklistItemResult {
                item: item.clone(),
                status: scan_one(item, idx + 1, output_text),
            })
            .collect(),
    }
}

fn scan_one(item: &str, n: usize, output: &str) -> ChecklistItemStatus {
    if output.contains(&format!("[x] {n}.")) || output.contains(&format!("[X] {n}.")) {
        return ChecklistItemStatus::Confirmed;
    }
    let mut anchors = Vec::new();
    let bracket = format!("[ ] {n}.");
    if let Some(p) = output.find(&bracket) {
        anchors.push(p);
    }
    // Match "N." at line start (with optional whitespace/markdown)
    let num_prefix = format!("{n}.");
    for (pos, _) in output.match_indices(&num_prefix) {
        let before = if pos > 0 { output.as_bytes()[pos - 1] } else { b'\n' };
        if before == b'\n' || before == b' ' || before == b'\t' || before == b'*' || pos == 0 {
            anchors.push(pos);
        }
    }
    if !item.is_empty() {
        for (pos, _) in output.match_indices(item) {
            anchors.push(pos);
        }
    }
    if anchors.is_empty() {
        let lower_out = output.to_lowercase();
        let lower_item = item.to_lowercase();
        let mut idx = 0usize;
        while let Some(rel) = lower_out[idx..].find(lower_item.as_str()) {
            anchors.push(idx + rel);
            idx += rel + 1;
        }
    }
    anchors.sort_unstable();
    anchors.dedup();
    for anchor in anchors {
        let lo = anchor.saturating_sub(NEAR);
        let hi = (anchor + item.len() + NEAR).min(output.len());
        if lo < hi {
            if let Some(s) = status_in_window(&output[lo..hi], anchor - lo) {
                return s;
            }
        }
    }
    ChecklistItemStatus::Missing
}

fn status_in_window(w: &str, ar: usize) -> Option<ChecklistItemStatus> {
    let lower = w.to_lowercase();
    let mut best: Option<(usize, usize, ChecklistItemStatus)> = None;
    for (word, st) in [("confirmed", ChecklistItemStatus::Confirmed), ("rejected", ChecklistItemStatus::Rejected)] {
        let mut s = 0usize;
        while let Some(rel) = lower[s..].find(word) {
            let pos = s + rel;
            let wl = word.len();
            let ok = (pos == 0 || !lower.as_bytes()[pos - 1].is_ascii_alphanumeric())
                && (pos + wl >= lower.len() || !lower.as_bytes()[pos + wl].is_ascii_alphanumeric());
            if ok {
                let d = pos.abs_diff(ar);
                if best.map_or(true, |(bd, bp, _)| d < bd || (d == bd && pos < bp)) {
                    best = Some((d, pos, st));
                }
            }
            s = pos + 1;
        }
    }
    best.map(|(_, _, st)| st)
}

#[cfg(test)]
#[path = "checklist_scan_tests.rs"]
mod tests;
