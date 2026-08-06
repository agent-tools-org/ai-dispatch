// Declared task-profile validation, category resolution, and persistence.
// Exports helpers used by dispatch preparation.
// Deps: RunArgs, task/store types, classifier, report-mode defaults.

use anyhow::Result;

use super::RunArgs;
use crate::agent;
use crate::store::Store;
use crate::types::{Task, TaskId, TaskProfileDeclaration, TaskRigor};

pub(super) fn validate_critical_rigor(args: &RunArgs) -> Result<()> {
    if args.declared_rigor != Some(TaskRigor::Critical) {
        return Ok(());
    }
    if args.verify.is_none() || !args.audit {
        anyhow::bail!("--rigor critical requires verification and cross-audit; pass --verify and --audit or configure both as project defaults");
    }
    Ok(())
}

/// `--egress local` is independent of rigor: only a loopback provider passes.
/// `--egress private-network` admits loopback or RFC1918/link-local endpoints.
pub(super) fn validate_egress(args: &RunArgs) -> Result<()> {
    if args.declared_egress.requires_local() {
        crate::agent::egress::require_local_egress(&args.agent_name)?;
    }
    if args.declared_egress.requires_private_network() {
        crate::agent::egress::require_private_network_egress(&args.agent_name)?;
    }
    Ok(())
}

pub(super) fn persist_declaration(store: &Store, task_id: &TaskId, args: &RunArgs) -> Result<()> {
    store.update_task_profile(task_id.as_str(), TaskProfileDeclaration {
        difficulty: args.declared_difficulty,
        budget: args.declared_budget,
        urgency: args.declared_urgency,
        rigor: args.declared_rigor,
    })
}

pub(super) fn apply_category_and_result_defaults(
    args: &mut RunArgs,
    task: &mut Task,
    had_explicit_result_file: bool,
) {
    let normalized_prompt = task.prompt.trim().to_lowercase();
    let profile = agent::classifier::classify(
        &task.prompt,
        agent::classifier::count_file_mentions(&normalized_prompt),
        task.prompt.chars().count(),
    );
    let category = args.kind.unwrap_or(profile.category);
    let report_output = crate::cmd::report_mode::apply_defaults(args, category);
    args.audit_report_mode = crate::cmd::report_mode::skips_dirty_enforcement(
        &args.prompt, args.read_only, category,
    );
    if report_output && !had_explicit_result_file && args.output.is_none() {
        args.result_file = Some(crate::cmd::report_mode::DEFAULT_AUDIT_RESULT_FILE.to_string());
    }
    task.category = Some(category.label().to_string());
}

pub(super) fn should_auto_result_file(args: &RunArgs, had_explicit_result_file: bool) -> bool {
    !had_explicit_result_file
        && args.output.is_none()
        && args.result_file.as_deref() == Some(crate::cmd::report_mode::DEFAULT_AUDIT_RESULT_FILE)
}
