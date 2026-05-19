// Audit report mode defaults for review-style tasks.
// Exports prompt detection, default result-file behavior, and prompt instructions.

use crate::agent::classifier::{TaskCategory, contains_any, contains_any_word};
use crate::cmd::run::RunArgs;

pub(crate) const DEFAULT_AUDIT_RESULT_FILE: &str = "result.md";

const AUDIT_TERMS: &[&str] = &[
    "audit",
    "cross-audit",
    "cross audit",
    "adversarial audit",
    "review",
    "code review",
    "peer review",
];
const STRUCTURED_FINDING_TERMS: &[&str] = &[
    "findings",
    "pass/fail",
    "severity",
    "evidence",
    "open questions",
];
const AUDIT_NOUN_PHRASES: &[&str] = &[
    "audit log",
    "audit trail",
    "add an audit",
    "add audit",
];

pub(crate) fn is_audit_report_task(
    prompt: &str,
    read_only: bool,
    category: TaskCategory,
    result_file: Option<&str>,
) -> bool {
    let normalized = prompt.trim().to_lowercase();
    let explicit_audit =
        (read_only || result_file.is_some()) && prompt_matches_audit_terms(&normalized);
    let structured_findings = contains_any(&normalized, STRUCTURED_FINDING_TERMS);
    explicit_audit
        || (read_only
            && matches!(
                category,
                TaskCategory::Research | TaskCategory::Documentation | TaskCategory::Debugging
            )
            && structured_findings)
}

pub(crate) fn apply_defaults(args: &mut RunArgs, category: TaskCategory) -> bool {
    if !is_audit_report_task(&args.prompt, args.read_only, category, args.result_file.as_deref()) {
        return false;
    }
    if args.result_file.is_none() && args.output.is_none() {
        args.result_file = Some(DEFAULT_AUDIT_RESULT_FILE.to_string());
    }
    true
}

pub(crate) fn task_result_file(task_id: &str) -> String {
    format!("result-{task_id}.md")
}

/// Cheap prompt-only check used by `aid show` to decide whether to surface
/// the "audit result missing" banner. Mirrors the explicit-audit branch of
/// `is_audit_report_task` without requiring a TaskCategory.
pub(crate) fn prompt_is_audit_report(prompt: &str) -> bool {
    let normalized = prompt.trim().to_lowercase();
    prompt_matches_audit_terms(&normalized)
}

pub(crate) fn instruction(
    prompt: &str,
    read_only: bool,
    category: TaskCategory,
    result_file: Option<&str>,
) -> Option<&'static str> {
    if !is_audit_report_task(prompt, read_only, category, result_file) {
        return None;
    }
    Some(
        "Write the final response as a Markdown audit report.\n\
Start with `## Findings`.\n\
List concrete findings first, ordered by severity, with file references and evidence.\n\
If there are no findings, say `No findings.` under `## Findings`.\n\
After findings, include `## Open Questions` only if needed.\n\
Do not include planning notes, tool logs, or meta-commentary in the final report.",
    )
}

fn prompt_matches_audit_terms(normalized_prompt: &str) -> bool {
    contains_any_word(normalized_prompt, AUDIT_TERMS)
        && !contains_any_word(normalized_prompt, AUDIT_NOUN_PHRASES)
}

#[cfg(test)]
mod tests {
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
    fn explicit_result_file_enables_audit_prompt_report_mode() {
        assert!(is_audit_report_task(
            "Audit the API surface",
            false,
            TaskCategory::ComplexImpl,
            Some("result.md"),
        ));
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
}
