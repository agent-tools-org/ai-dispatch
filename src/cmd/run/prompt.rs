// Prompt assembly and run helpers for `aid run`.
// Deps: resolved project context, store, skills, team, and execution helpers.
use crate::cmd::summary::{CompletionSummary, format_summary_for_injection};
use crate::{agent, project, store::Store, team, templates, toolbox, types::*};
use anyhow::Result;
use serde_json;
mod prompt_context;
#[path = "prompt_project.rs"]
mod prompt_project;
#[path = "output.rs"]
mod run_output;
#[path = "scope.rs"]
mod run_scope;
#[path = "verify.rs"]
mod run_verify;
pub(super) use run_output::{
    ResultDelivery, clean_output_if_jsonl, extract_output_fallback_from_path,
    fill_empty_output_from_log, output_file_instruction, persist_result_file,
};
pub(super) use run_scope::warn_agent_committed_files_outside_scope;
pub(super) use run_verify::{
    maybe_auto_retry_after_checklist_miss_impl, maybe_auto_retry_after_verify_failure_impl,
    maybe_verify_impl, record_verify_not_run,
};
#[path = "process.rs"]
mod run_process;
#[path = "prompt_helpers.rs"]
mod run_prompt_helpers;
use super::RunArgs;
pub(super) use run_process::*;
pub(super) use run_prompt_helpers::*;
#[path = "prompt_injections.rs"]
mod injections;
use injections::*;
#[path = "prompt_auxiliary.rs"]
mod auxiliary;
use auxiliary::*;
const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";

pub(crate) struct PromptBundle {
    pub effective_prompt: String,
    pub context_files: Vec<String>,
    pub prompt_tokens: i64,
    pub injected_memory_ids: Vec<String>,
}

pub(super) fn build_prompt_bundle(
    store: &Store,
    args: &RunArgs,
    agent_kind: &AgentKind,
    workgroup: Option<&Workgroup>,
    requested_skills: &[String],
    current_task_id: &str,
    detected_project: Option<&project::ProjectConfig>,
    project_root: Option<&std::path::Path>,
) -> Result<PromptBundle> {
    let (composed, context_files, category, suppress_implementation_scaffolding) =
        prepare_composed_prompt(store, args, agent_kind, workgroup)?;
    let mut omissions = composed.omissions;
    let mut effective_prompt = inject_related_context(
        store,
        args,
        agent_kind,
        current_task_id,
        requested_skills,
        suppress_implementation_scaffolding,
        composed.text,
        &mut omissions,
    )?;
    let injected_memory_ids = inject_stored_context(
        store,
        args,
        project_root,
        detected_project,
        category,
        &mut effective_prompt,
        &mut omissions,
    )?;

    effective_prompt =
        inject_delivery_context(args, agent_kind, category, effective_prompt, &mut omissions)?;
    record_prompt_omissions(store, current_task_id, &omissions)?;
    let prompt_tokens = templates::estimate_tokens(&effective_prompt) as i64;
    Ok(PromptBundle {
        effective_prompt,
        context_files,
        prompt_tokens,
        injected_memory_ids,
    })
}

fn prepare_composed_prompt(
    store: &Store,
    args: &RunArgs,
    agent_kind: &AgentKind,
    workgroup: Option<&Workgroup>,
) -> Result<(
    crate::workgroup::ComposedPrompt,
    Vec<String>,
    agent::classifier::TaskCategory,
    bool,
)> {
    let (mut composed, context_files, prompt) =
        compose_task_context(store, args, agent_kind, workgroup)?;
    let mut task_profile = agent::classifier::classify(
        &prompt,
        agent::classifier::count_file_mentions(&prompt),
        prompt.len(),
    );
    task_profile.category = effective_category(task_profile.category, args.kind);
    let suppress_implementation_scaffolding =
        crate::cmd::report_mode::suppresses_implementation_scaffolding(
            &prompt,
            args.read_only,
            args.kind,
        );
    let (edit_guard, milestone_instr) = templates::shared_system_fragments(&prompt);
    if let Some(guard) = edit_guard {
        composed.text = format!("{guard}{}", composed.text);
    }
    composed.text.push_str(milestone_instr);
    if !suppress_implementation_scaffolding {
        composed.text.push_str(templates::git_staging_guard());
    }

    Ok((
        composed,
        context_files,
        task_profile.category,
        suppress_implementation_scaffolding,
    ))
}

fn compose_task_context(
    store: &Store,
    args: &RunArgs,
    agent_kind: &AgentKind,
    workgroup: Option<&Workgroup>,
) -> Result<(crate::workgroup::ComposedPrompt, Vec<String>, String)> {
    let (file_context, context_files) = build_context_flags(agent_kind, &args.context)?;
    let milestones = if let Some(group_id) = args.group.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let findings = if let Some(group_id) = args.group.as_deref() {
        store.list_findings(group_id)?
    } else {
        vec![]
    };
    let prompt = resolve_prompt(&args.prompt, args.template.as_deref())?;
    let composed = crate::workgroup::compose_prompt(
        &prompt,
        file_context.as_deref(),
        workgroup,
        &milestones,
        &findings,
    );
    Ok((composed, context_files, prompt))
}

#[cfg(test)]
#[path = "prompt_tests.rs"]
mod tests;

/// The category that drives toolbox filtering and skill auto-apply: a declared
/// `--kind` beats the keyword guess.
///
/// The declaration was being dropped entirely. `classifier::classify` only ever
/// saw the prompt text, so `aid run agy --kind research ...` still announced
/// "filtered by frontend" and chose its toolbox and skills from the guess.
/// Accepting a declaration and then ignoring it is worse than not offering the
/// flag at all, because the caller believes it decided something.
fn effective_category(
    guessed: crate::agent::classifier::TaskCategory,
    declared: Option<crate::agent::classifier::TaskCategory>,
) -> crate::agent::classifier::TaskCategory {
    declared.unwrap_or(guessed)
}

#[cfg(test)]
mod effective_category_tests {
    use super::effective_category;
    use crate::agent::classifier::TaskCategory;

    #[test]
    fn a_declared_kind_overrides_the_guess() {
        assert_eq!(
            effective_category(TaskCategory::Frontend, Some(TaskCategory::Research)),
            TaskCategory::Research
        );
    }

    #[test]
    fn the_guess_stands_when_nothing_is_declared() {
        assert_eq!(
            effective_category(TaskCategory::Frontend, None),
            TaskCategory::Frontend
        );
    }

    /// Declaring the same category the guess produced must be a no-op rather
    /// than an error, so a caller can declare defensively.
    #[test]
    fn declaring_the_guessed_kind_changes_nothing() {
        assert_eq!(
            effective_category(TaskCategory::Testing, Some(TaskCategory::Testing)),
            TaskCategory::Testing
        );
    }
}
