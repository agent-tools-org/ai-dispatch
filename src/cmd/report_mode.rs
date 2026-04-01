// Audit report mode defaults for review-style tasks.
// Exports prompt detection, default result-file behavior, and prompt instructions.

use crate::agent::classifier::{TaskCategory, contains_any};
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

pub(crate) fn is_audit_report_task(
    prompt: &str,
    read_only: bool,
    category: TaskCategory,
) -> bool {
    let normalized = prompt.trim().to_lowercase();
    let explicit_audit = contains_any(&normalized, AUDIT_TERMS);
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
    if !is_audit_report_task(&args.prompt, args.read_only, category) {
        return false;
    }
    if args.result_file.is_none() {
        args.result_file = Some(DEFAULT_AUDIT_RESULT_FILE.to_string());
    }
    true
}

pub(crate) fn instruction(prompt: &str, read_only: bool, category: TaskCategory) -> Option<&'static str> {
    if !is_audit_report_task(prompt, read_only, category) {
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_prompt_enables_report_mode() {
        assert!(is_audit_report_task(
            "Cross-audit the split routing fix and produce findings.",
            true,
            TaskCategory::Research,
        ));
    }

    #[test]
    fn read_only_findings_prompt_enables_report_mode() {
        assert!(is_audit_report_task(
            "Check PASS/FAIL with evidence and list findings.",
            true,
            TaskCategory::Documentation,
        ));
    }

    #[test]
    fn generic_research_prompt_does_not_enable_report_mode() {
        assert!(!is_audit_report_task(
            "Explain how routing tiers work.",
            true,
            TaskCategory::Research,
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
}
