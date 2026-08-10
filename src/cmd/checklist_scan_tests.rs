// Tests the exact checklist response contract without proximity heuristics.
// Covers confirmed, rejected, missing, case folding, and false-positive prose.

use super::{ChecklistItemStatus, scan_checklist};

fn items(values: &[&str]) -> Vec<String> {
    values.iter().map(|value| (*value).to_string()).collect()
}

#[test]
fn explicit_responses_are_addressed() {
    let checklist = items(&["a", "b", "c"]);
    let output = concat!(
        "CHECKLIST 1: CONFIRMED — evidence\n",
        "checklist 2: rejected — reason\n",
        "CHECKLIST 3: CONFIRMED: evidence"
    );
    let result = scan_checklist(&checklist, output);

    assert!(result.all_addressed());
    assert_eq!(result.items[1].status, ChecklistItemStatus::Rejected);
    assert_eq!(result.summary(), "3/3 addressed (2 confirmed, 1 rejected)");
}

#[test]
fn missing_explicit_response_is_reported() {
    let checklist = items(&["present", "absent"]);
    let result = scan_checklist(
        &checklist,
        "CHECKLIST 1: CONFIRMED — evidence\nnothing about the other",
    );

    assert!(!result.all_addressed());
    assert_eq!(result.missing_items(), vec!["absent"]);
}

#[test]
fn prose_and_legacy_checkbox_do_not_invent_a_response() {
    let checklist = items(&["task"]);
    for output in [
        "task confirmed somewhere in prose",
        "[x] 1. task done",
        "the word CHECKLIST 1: CONFIRMED is quoted mid-line",
    ] {
        let result = scan_checklist(&checklist, output);
        assert_eq!(result.items[0].status, ChecklistItemStatus::Missing);
    }
}

#[test]
fn empty_checklist_is_addressed() {
    let result = scan_checklist(&[], "");

    assert!(result.all_addressed());
    assert_eq!(result.summary(), "0/0 addressed (0 confirmed, 0 rejected)");
}
