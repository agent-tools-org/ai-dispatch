// Tests that a declared writable kind overrides audit keyword inference.
// Exports: none.
// Deps: super report_mode helpers, RunArgs and TaskCategory.

use super::*;

const AUDIT_WORDED_BRIEF: &str =
    "Build the offline eval for cross-audit detection; report findings with severity and evidence.";

fn args(kind: Option<TaskCategory>, read_only: bool, result_file: Option<&str>) -> RunArgs {
    RunArgs {
        prompt: AUDIT_WORDED_BRIEF.to_string(),
        kind,
        read_only,
        result_file: result_file.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn undeclared_kind_still_infers_report_mode_from_wording() {
    let mut run = args(None, false, None);
    assert!(apply_defaults(&mut run, TaskCategory::Research));
}

#[test]
fn declared_writable_kind_blocks_report_defaults() {
    for kind in [
        TaskCategory::SimpleEdit,
        TaskCategory::ComplexImpl,
        TaskCategory::Frontend,
        TaskCategory::Testing,
        TaskCategory::Refactoring,
    ] {
        let mut run = args(Some(kind), false, None);
        assert!(!apply_defaults(&mut run, kind), "{kind:?}");
        assert_eq!(run.result_file, None, "{kind:?}");
    }
}

#[test]
fn retry_with_persisted_result_file_gets_no_report_instruction() {
    let result_file = Some("result-t-1.md");
    let undeclared = instruction(AUDIT_WORDED_BRIEF, false, TaskCategory::ComplexImpl, result_file, None);
    assert!(undeclared.is_some(), "control: a persisted result file re-triggers report mode");
    let declared = instruction(
        AUDIT_WORDED_BRIEF, false, TaskCategory::ComplexImpl, result_file, Some(TaskCategory::ComplexImpl),
    );
    assert!(declared.is_none());
}

#[test]
fn declared_report_kind_keeps_keyword_inference() {
    let mut run = args(Some(TaskCategory::Research), false, None);
    assert!(apply_defaults(&mut run, TaskCategory::Research));
}

#[test]
fn read_only_overrides_declared_writable_kind() {
    let mut run = args(Some(TaskCategory::ComplexImpl), true, None);
    assert!(apply_defaults(&mut run, TaskCategory::ComplexImpl));
}

#[test]
fn declared_writable_kind_keeps_implementation_scaffolding() {
    let prompt = "Read-only audit of the parser; report findings.";
    assert!(suppresses_implementation_scaffolding(prompt, false, None), "control");
    assert!(!suppresses_implementation_scaffolding(prompt, false, Some(TaskCategory::ComplexImpl)));
}
