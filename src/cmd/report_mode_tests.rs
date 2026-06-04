// Tests for audit report-mode detection and defaults.
// Exports: none.
// Deps: super report_mode helpers and TaskCategory.

use super::*;

#[test]
fn audit_prompt_enables_report_mode() {
    assert!(is_audit_report_task(
        "Cross-audit the split routing fix and produce findings.",
        true,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn read_only_findings_prompt_enables_report_mode() {
    assert!(is_audit_report_task(
        "Check PASS/FAIL with evidence and list findings.",
        true,
        TaskCategory::Documentation,
        None,
    ));
}

#[test]
fn generic_research_prompt_does_not_enable_report_mode() {
    assert!(!is_audit_report_task(
        "Explain how routing tiers work.",
        true,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn write_prompts_do_not_enable_report_mode_without_result_file() {
    for prompt in [
        "Add an audit log feature",
        "Implement the requested fix",
        "Redesign the audit subsystem",
        "Investigate and fix the crash",
    ] {
        assert!(!is_audit_report_task(
            prompt,
            false,
            TaskCategory::ComplexImpl,
            None,
        ));
    }
}

#[test]
fn read_only_audit_prompts_enable_report_mode() {
    assert!(is_audit_report_task(
        "Cross-audit the calldata encoding",
        true,
        TaskCategory::Research,
        None,
    ));
    assert!(is_audit_report_task(
        "Code review of changes",
        true,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn read_only_audit_prompts_handle_case_spacing_and_punctuation() {
    for prompt in [
        "AUDIT THE API SURFACE",
        "  cross-audit the calldata",
        "cross-audit, the calldata",
    ] {
        assert!(is_audit_report_task(
            prompt,
            true,
            TaskCategory::Research,
            None,
        ));
    }
}

#[test]
fn audit_noun_phrase_does_not_mask_explicit_audit_term() {
    assert!(is_audit_report_task(
        "cross-audit the audit log feature",
        true,
        TaskCategory::Research,
        None,
    ));
    assert!(prompt_is_audit_report("cross-audit the audit log feature"));
}

#[test]
fn audit_log_feature_prompt_does_not_enable_report_mode() {
    assert!(!is_audit_report_task(
        "Implement an audit log feature",
        true,
        TaskCategory::Research,
        None,
    ));
    assert!(!prompt_is_audit_report("Implement an audit log feature"));
}

#[test]
fn explicit_result_file_enables_audit_prompt_report_mode() {
    assert!(is_audit_report_task(
        "Audit the API surface",
        false,
        TaskCategory::ComplexImpl,
        Some("result.md"),
    ));
}

#[test]
fn non_read_only_cross_audit_enables_report_mode() {
    assert!(is_audit_report_task(
        "Cross-audit the split routing fix and produce findings.",
        false,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn non_read_only_bare_audit_prompt_does_not_enable_report_mode() {
    assert!(!is_audit_report_task(
        "Redesign the audit subsystem",
        false,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn adversarial_auditor_prompt_enables_report_instruction_only() {
    let prompt = "You are an ADVERSARIAL, read-only code auditor. Do NOT modify any code. Audit the report mode feature on the CURRENT git branch against base main.";

    assert!(is_audit_report_task(
        prompt,
        false,
        TaskCategory::Research,
        None,
    ));
    assert!(instruction(prompt, false, TaskCategory::Research, None).is_some());
    assert!(!skips_dirty_enforcement(prompt, false, TaskCategory::Research));
}

#[test]
fn audit_against_baseline_enables_report_mode() {
    assert!(is_audit_report_task(
        "Audit the routing feature against main and report findings.",
        false,
        TaskCategory::Research,
        None,
    ));
}

#[test]
fn counter_examples_stay_out_of_report_mode() {
    for prompt in ["add an audit log", "review and refactor this module"] {
        assert!(!is_audit_report_task(
            prompt,
            false,
            TaskCategory::ComplexImpl,
            None,
        ));
    }
}

#[test]
fn apply_defaults_sets_result_file_once() {
    let mut args = RunArgs {
        prompt: "Review the implementation and list findings.".to_string(),
        read_only: true,
        ..Default::default()
    };

    assert!(apply_defaults(&mut args, TaskCategory::Research));
    assert_eq!(args.result_file.as_deref(), Some(DEFAULT_AUDIT_RESULT_FILE));
}

#[test]
fn apply_defaults_skips_result_file_when_output_is_set() {
    let mut args = RunArgs {
        prompt: "Review the implementation and list findings.".to_string(),
        read_only: true,
        output: Some("report.md".to_string()),
        ..Default::default()
    };

    assert!(apply_defaults(&mut args, TaskCategory::Research));
    assert_eq!(args.result_file, None);
}

#[test]
fn task_result_file_uses_task_id_suffix() {
    assert_eq!(task_result_file("t-123"), "result-t-123.md");
}
