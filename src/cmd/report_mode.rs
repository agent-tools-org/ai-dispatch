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
const AUTO_REPORT_AUDIT_TERMS: &[&str] = &[
    "cross-audit",
    "cross audit",
    "adversarial audit",
    "code review",
    "peer review",
];
const AUDITOR_ROLE_PREFIXES: &[&str] = &[
    "you are a",
    "you are an",
    "you are the",
    "act as a",
    "act as an",
    "act as the",
];
const AUDIT_BASELINES: &[&str] = &["main", "base", "baseline"];

pub(crate) fn is_audit_report_task(
    prompt: &str,
    read_only: bool,
    category: TaskCategory,
    result_file: Option<&str>,
) -> bool {
    let normalized = prompt.trim().to_lowercase();
    let explicit_audit =
        (read_only || result_file.is_some()) && prompt_matches_audit_terms(&normalized);
    let auto_audit_report = matches!(
        category,
        TaskCategory::Research | TaskCategory::Documentation | TaskCategory::Debugging
    ) && prompt_matches_auto_report_terms(&normalized);
    let strong_audit_intent = prompt_matches_strong_audit_intent(&normalized);
    let structured_findings = contains_any(&normalized, STRUCTURED_FINDING_TERMS);
    explicit_audit
        || auto_audit_report
        || strong_audit_intent
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

/// Narrow predicate: should this task skip dirty-worktree enforcement?
/// Only genuine report-only tasks qualify - not a write-capable task that merely
/// has --result-file plus a broad audit word like "review".
pub(crate) fn skips_dirty_enforcement(prompt: &str, read_only: bool, category: TaskCategory) -> bool {
    if read_only { return true; }
    let normalized = prompt.trim().to_lowercase();
    matches!(category, TaskCategory::Research | TaskCategory::Documentation | TaskCategory::Debugging)
        && prompt_matches_auto_report_terms(&normalized)
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
    let stripped = strip_audit_noun_phrases(normalized_prompt);
    contains_any_word(&stripped, AUDIT_TERMS)
}

fn prompt_matches_auto_report_terms(normalized_prompt: &str) -> bool {
    let stripped = strip_audit_noun_phrases(normalized_prompt);
    contains_any_word(&stripped, AUTO_REPORT_AUDIT_TERMS)
}

fn prompt_matches_strong_audit_intent(normalized_prompt: &str) -> bool {
    let stripped = strip_audit_noun_phrases(normalized_prompt);
    prompt_has_auditor_role(&stripped) || prompt_has_audit_against_baseline(&stripped)
}

fn prompt_has_auditor_role(stripped_prompt: &str) -> bool {
    (contains_any(stripped_prompt, AUDITOR_ROLE_PREFIXES)
        || contains_any_word(stripped_prompt, &["adversarial"]))
        && contains_any_word(stripped_prompt, &["auditor", "code auditor"])
}

fn prompt_has_audit_against_baseline(stripped_prompt: &str) -> bool {
    let Some(audit_at) = stripped_prompt.find("audit") else { return false; };
    let after_audit = &stripped_prompt[audit_at..];
    contains_any_word(after_audit, &["audit"])
        && contains_any(after_audit, &[" against "])
        && contains_any_word(after_audit, AUDIT_BASELINES)
}

fn strip_audit_noun_phrases(normalized_prompt: &str) -> String {
    let mut stripped = normalized_prompt.to_string();
    for phrase in AUDIT_NOUN_PHRASES {
        stripped = stripped.replace(phrase, " ");
    }
    stripped
}

#[cfg(test)] #[path = "report_mode_tests.rs"] mod tests;
