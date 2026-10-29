// Workgroup composition tests for context bounds and shared entries.
// Uses the composer and private auxiliary bounding helper.
use super::compose_prompt;
use crate::types::{Finding, Workgroup, WorkgroupId};
use chrono::Local;

fn make_group(shared_context: &str) -> Workgroup {
    Workgroup {
        id: WorkgroupId("wg-demo".to_string()),
        name: "dispatch".to_string(),
        shared_context: shared_context.to_string(),
        created_by: None,
        created_at: Local::now(),
        updated_at: Local::now(),
    }
}

#[test]
fn returns_prompt_when_no_context_exists() {
    assert_eq!(compose_prompt("ship it", None, None, &[], &[]).text, "ship it");
}

#[test]
fn includes_shared_and_file_context() {
    let prompt = compose_prompt(
        "ship it",
        Some("fn main() {}"),
        Some(&make_group("Rust workspace with shared target dir.")),
        &[],
        &[],
    )
    .text;
    assert!(prompt.contains("[Shared Context: dispatch]"));
    assert!(prompt.contains("[Context]"));
    assert!(prompt.contains("[Task]"));
    assert!(prompt.contains("ship it"));
}

#[test]
fn includes_shared_findings() {
    let milestones = vec![
        ("t-1000".to_string(), "finding one".to_string()),
        ("t-1001".to_string(), "finding two".to_string()),
    ];
    let prompt = compose_prompt("ship it", None, None, &milestones, &[]).text;

    assert!(prompt.contains("[Peer Milestones]"));
    assert!(prompt.contains("- [t-1000] finding one"));
    assert!(prompt.contains("- [t-1001] finding two"));
    assert!(prompt.contains("[Task]"));
}

#[test]
fn short_context_unchanged() {
    let short = "hello world\nline two";
    assert_eq!(super::bound_auxiliary(short, "Context", &mut Vec::new()), short);
}

#[test]
fn long_context_compressed() {
    let lines: Vec<String> = (0..500).map(|i| format!("line {i}: some content here")).collect();
    let long = lines.join("\n");
    let compressed = super::bound_auxiliary(&long, "Context", &mut Vec::new());
    assert!(compressed.len() <= super::MAX_CONTEXT_BYTES + 200);
    assert!(compressed.contains("[... "));
    assert!(compressed.contains("characters omitted"));
}

#[test]
fn compression_preserves_recent() {
    let mut lines: Vec<String> = (0..500).map(|i| format!("line {i}")).collect();
    lines.push("IMPORTANT_LAST_LINE".to_string());
    let long = lines.join("\n");
    let compressed = super::bound_auxiliary(&long, "Context", &mut Vec::new());
    assert!(compressed.contains("IMPORTANT_LAST_LINE"));
}

#[test]
fn includes_findings_from_investigation() {
    let findings = vec![Finding {
        id: 1,
        workgroup_id: "wg-1".to_string(),
        content: "gamma can be zero".to_string(),
        source_task_id: Some("t-100".to_string()),
        created_at: Local::now(),
        ..Default::default()
    }];
    let prompt = compose_prompt("investigate", None, None, &[], &findings).text;
    assert!(prompt.contains("[Shared Findings]"));
    assert!(prompt.contains("gamma can be zero"));
}

#[test]
fn finding_content_truncated_at_500() {
    let long = "a".repeat(1000);
    let findings = vec![Finding {
        id: 1,
        workgroup_id: "wg-1".to_string(),
        content: long,
        source_task_id: Some("t-100".to_string()),
        created_at: Local::now(),
        ..Default::default()
    }];

    let prompt = compose_prompt("investigate", None, None, &[], &findings).text;

    assert!(prompt.contains(&format!("- [t-100] {}...", "a".repeat(497))));
    assert!(!prompt.contains(&"a".repeat(498)));
}

#[test]
fn milestone_content_truncated_at_500() {
    let milestones = vec![("t-1000".to_string(), "b".repeat(1000))];

    let prompt = compose_prompt("ship it", None, None, &milestones, &[]).text;

    assert!(prompt.contains(&format!("- [t-1000] {}...", "b".repeat(497))));
    assert!(!prompt.contains(&"b".repeat(498)));
}

#[test]
fn short_finding_unchanged() {
    let findings = vec![Finding {
        id: 1,
        workgroup_id: "wg-1".to_string(),
        content: "short finding".to_string(),
        source_task_id: Some("t-100".to_string()),
        created_at: Local::now(),
        ..Default::default()
    }];

    let prompt = compose_prompt("investigate", None, None, &[], &findings).text;

    assert!(prompt.contains("- [t-100] short finding"));
}
