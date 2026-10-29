// Regression coverage for task preservation and explicit auxiliary omissions.
// Uses the shared composer so run, show, and export follow the same boundary.
use super::compose_prompt;

fn long_task() -> String {
    (0..100)
        .map(|i| format!("task line {i:03}: {}", "x".repeat(50)))
        .collect::<Vec<_>>()
        .join("\n")
}

#[test]
fn task_over_6500_chars_keeps_every_line_with_context() {
    let task = long_task();
    assert!(task.len() > 6500);
    let composed = compose_prompt(&task, Some("caller context"), None, &[], &[]).text;
    assert_eq!(composed.split_once("[Task]\n").unwrap().1, task);
}

#[test]
fn retry_feedback_survives_long_original_brief_with_context() {
    let feedback = format!("Fix the previous result.\n{}", "feedback\n".repeat(100));
    let task = format!(
        "[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{}",
        long_task()
    );
    let composed = compose_prompt(&task, Some(&"context\n".repeat(1000)), None, &[], &[]).text;
    assert_eq!(composed.split_once("[Task]\n").unwrap().1, task);
}

#[test]
fn auxiliary_context_is_bounded_with_an_in_place_omission_note() {
    let context = "auxiliary line\n".repeat(1000);
    let composed = compose_prompt("complete task", Some(&context), None, &[], &[]).text;
    let (auxiliary, task) = composed.split_once("[Task]\n").unwrap();
    assert!(auxiliary.len() <= 6200);
    assert!(auxiliary.contains("omitted"));
    assert_eq!(task, "complete task");
}

#[test]
fn each_auxiliary_section_has_its_own_bound_and_omission_note() {
    let store = crate::store::Store::open_memory().unwrap();
    let group = store
        .create_workgroup("test", &"shared\n".repeat(2000), None, None)
        .unwrap();
    let findings = (0..100)
        .map(|i| finding(format!("finding {i}: {}", "f".repeat(100))))
        .collect::<Vec<_>>();
    let milestones = (0..100)
        .map(|i| (format!("t-{i}"), "m".repeat(100)))
        .collect::<Vec<_>>();
    let composed = compose_prompt(
        "whole task",
        Some(&"context\n".repeat(2000)),
        Some(&group),
        &milestones,
        &findings,
    );
    assert_eq!(composed.omissions.len(), 4);
    for (label, next) in [
        ("Shared Context: test", "Context"),
        ("Context", "Shared Findings"),
        ("Shared Findings", "Peer Milestones"),
        ("Peer Milestones", "Task"),
    ] {
        let section = composed
            .text
            .split_once(&format!("[{label}]\n"))
            .unwrap()
            .1
            .split_once(&format!("[{next}]\n"))
            .unwrap()
            .0;
        assert!(section.len() <= 6200, "{label}");
        assert!(section.contains("characters omitted"), "{label}");
    }
    assert!(composed.text.contains("finding 99:"));
    assert!(composed.text.contains("- [t-99]"));
    assert!(composed.text.ends_with("[Task]\nwhole task"));
}

#[test]
fn single_line_unicode_context_is_bounded_without_cutting_task_characters() {
    let composed = compose_prompt("task\n末尾\n", Some(&"界".repeat(4000)), None, &[], &[]);
    assert_eq!(composed.omissions.len(), 1);
    assert!(composed.text.len() <= 6300);
    assert!(composed.text.contains("characters omitted"));
    assert!(composed.text.ends_with("[Task]\ntask\n末尾\n"));
}

#[test]
fn entry_truncation_is_noted_and_reported_even_below_section_bound() {
    let composed = compose_prompt(
        "task",
        None,
        None,
        &[("t-1".into(), "m".repeat(1000))],
        &[finding("f".repeat(1000))],
    );
    assert_eq!(composed.omissions.len(), 2);
    assert_eq!(
        composed.text.matches("503 characters of entry omitted").count(),
        2
    );
    assert!(
        composed
            .omissions
            .iter()
            .all(|note| note.contains("503 characters of entries omitted"))
    );
}

fn finding(content: String) -> crate::types::Finding {
    crate::types::Finding {
        id: 1,
        workgroup_id: "wg-test".into(),
        content,
        source_task_id: Some("t-1".into()),
        created_at: chrono::Local::now(),
        ..Default::default()
    }
}
