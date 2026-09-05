// Aggregate deterministic run-option errors after project defaults, before task creation.
// Exports validate_run_options(); depends on RunArgs, task rigor, typed diagnostics.

use crate::cmd::run::RunArgs;
use crate::types::{TaskRigor, verify_required};
use super::{Issue, Rejection};

pub(crate) fn validate_run_options(args: &RunArgs) -> anyhow::Result<()> {
    let mut issues = Vec::new();
    if args.read_only && args.worktree.is_some() {
        issues.push(Issue::new("read_only_worktree",
            "--read-only cannot be used with --worktree.",
            "To inspect an existing checkout, replace --worktree <branch> with --dir <checkout-path>; keep --read-only."));
    }
    if args.sandbox && args.container.is_some() {
        issues.push(Issue::new("sandbox_container",
            "--sandbox cannot be combined with --container (including a project container default).",
            "Choose one execution environment; remove the container default when using --sandbox."));
    }
    if args.audit && args.no_audit {
        issues.push(Issue::new("audit_no_audit", "--audit conflicts with --no-audit.",
            "Keep --audit to schedule post-task cross-audit, or --no-audit to disable it."));
    }
    validate_rigor(args, &mut issues);
    validate_iteration(args, &mut issues);
    if issues.is_empty() { return Ok(()); }
    Err(Rejection(issues).into())
}

fn validate_rigor(args: &RunArgs, issues: &mut Vec<Issue>) {
    if args.declared_rigor != Some(TaskRigor::Critical) { return; }
    if !verify_required(args.verify.as_deref()) || !args.audit {
        issues.push(Issue::new("critical_proof_required",
            "--rigor critical requires verification and cross-audit.",
            "Pass --verify <command> and --audit, or configure both as project defaults. Disabled verification (empty, none, false, skip) does not satisfy critical rigor. --audit is a separate post-task check."));
    }
}

fn validate_iteration(args: &RunArgs, issues: &mut Vec<Issue>) {
    if args.iterate == Some(0) {
        issues.push(Issue::new("iterate_count", "--iterate must be at least 1.",
            "Use --iterate <positive-count> --eval <command>."));
    }
    if args.iterate.is_some() && args.eval.is_none() {
        issues.push(Issue::new("iterate_eval_required", "--iterate requires --eval.",
            "Add --eval <command>; exit 0 means the iteration succeeded."));
    }
    if args.iterate.is_none() && (args.eval.is_some() || args.eval_feedback_template.is_some()) {
        issues.push(Issue::new("eval_iterate_required", "--eval and --eval-feedback-template require --iterate.",
            "Add --iterate <positive-count>, or remove the evaluation options."));
    }
    if args.eval.as_ref().is_some_and(|eval| eval.trim().is_empty()) {
        issues.push(Issue::new("eval_empty", "--eval cannot be empty.", "Provide a non-empty evaluation command."));
    }
}
