// Unit tests for checklist output scanning (CONFIRMED / REJECTED / Missing).

use super::{scan_checklist, ChecklistItemStatus};

fn items(s: &[&str]) -> Vec<String> {
    s.iter().map(|x| (*x).to_string()).collect()
}

#[test]
fn all_confirmed_all_addressed() {
    let checklist = items(&["a", "b", "c"]);
    let out = "a CONFIRMED\nb confirmed\nc CONFIRMED";
    let r = scan_checklist(&checklist, out);
    assert!(r.all_addressed());
    assert!(r.missing_items().is_empty());
}

#[test]
fn one_missing_reported() {
    let checklist = items(&["present", "absent"]);
    let out = "present CONFIRMED\nnothing about the other";
    let r = scan_checklist(&checklist, out);
    assert!(!r.all_addressed());
    assert_eq!(r.missing_items(), vec!["absent"]);
}

#[test]
fn mixed_confirmed_rejected_all_addressed() {
    let checklist = items(&["x", "y", "z"]);
    let out = "x CONFIRMED\ny REJECTED\nz confirmed";
    let r = scan_checklist(&checklist, out);
    assert!(r.all_addressed());
    assert!(r.missing_items().is_empty());
}

#[test]
fn empty_checklist_all_addressed() {
    let r = scan_checklist(&[], "");
    assert!(r.all_addressed());
    assert_eq!(r.summary(), "0/0 addressed (0 confirmed, 0 rejected)");
}

#[test]
fn case_insensitive_keywords() {
    let checklist = items(&["only"]);
    let r = scan_checklist(&checklist, "confirmed for only item");
    assert!(r.all_addressed());
    let r2 = scan_checklist(&checklist, "only: rejected — cannot do");
    assert_eq!(r2.items[0].status, ChecklistItemStatus::Rejected);
}

#[test]
fn numbered_bracket_line_then_confirmed() {
    let checklist = items(&["item"]);
    let out = "[ ] 1. item\nCONFIRMED";
    let r = scan_checklist(&checklist, out);
    assert!(r.all_addressed());
    assert_eq!(r.items[0].status, ChecklistItemStatus::Confirmed);
}

#[test]
fn checkbox_x_marked_confirmed() {
    let checklist = items(&["task"]);
    let out = "[x] 1. task done";
    let r = scan_checklist(&checklist, out);
    assert_eq!(r.items[0].status, ChecklistItemStatus::Confirmed);
}

#[test]
fn summary_counts() {
    let checklist = items(&["a", "b", "c"]);
    let r = scan_checklist(&checklist, "a CONFIRMED\nb REJECTED\nc CONFIRMED");
    assert_eq!(r.summary(), "3/3 addressed (2 confirmed, 1 rejected)");
}
