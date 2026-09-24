// Prompt assembly and run helpers for `aid run`.
// Deps: resolved project context, store, skills, team, and execution helpers.
use anyhow::Result;
use serde_json;
use crate::{agent, project, store::Store, templates, team, toolbox, types::*};
use crate::cmd::summary::{format_summary_for_injection, CompletionSummary};
mod prompt_context;
#[path = "prompt_project.rs"]
mod prompt_project;
#[path = "output.rs"]
mod run_output;
#[path = "verify.rs"]
mod run_verify;
#[path = "scope.rs"]
mod run_scope;
pub(super) use run_output::{
    ResultDelivery, clean_output_if_jsonl, extract_output_fallback_from_path,
    fill_empty_output_from_log, output_file_instruction, persist_result_file,
};
pub(super) use run_scope::warn_agent_committed_files_outside_scope;
pub(super) use run_verify::{maybe_auto_retry_after_checklist_miss_impl, maybe_auto_retry_after_verify_failure_impl, maybe_cleanup_fast_fail_impl, maybe_verify_impl, record_verify_not_run};
#[path = "process.rs"]
mod run_process;
#[path = "prompt_helpers.rs"]
mod run_prompt_helpers;
pub(super) use run_process::*;
pub(super) use run_prompt_helpers::*;
use super::RunArgs;
const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";
const PROMPT_TOKEN_LIMIT: usize = 30_000;

pub(crate) struct PromptBundle { pub effective_prompt: String, pub context_files: Vec<String>, pub prompt_tokens: i64, pub injected_memory_ids: Vec<String> }

pub(super) fn build_prompt_bundle(store: &Store, args: &RunArgs, agent_kind: &AgentKind, workgroup: Option<&Workgroup>, requested_skills: &[String], current_task_id: &str, detected_project: Option<&project::ProjectConfig>, project_root: Option<&std::path::Path>) -> Result<PromptBundle> {
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
    let mut task_profile = agent::classifier::classify(
        &prompt,
        agent::classifier::count_file_mentions(&prompt),
        prompt.len(),
    );
    task_profile.category = effective_category(task_profile.category, args.kind);
    let suppress_implementation_scaffolding = crate::cmd::report_mode::suppresses_implementation_scaffolding(
        &prompt, args.read_only, args.kind,
    );
    let task_category_label = task_profile.category.label();
    let mut effective_prompt = crate::workgroup::compose_prompt(
        &prompt,
        file_context.as_deref(),
        workgroup,
        &milestones,
        &findings,
    );
    let (edit_guard, milestone_instr) = templates::shared_system_fragments(&prompt);
    if let Some(guard) = edit_guard { effective_prompt = format!("{guard}{effective_prompt}"); }
    effective_prompt.push_str(milestone_instr);
    if !suppress_implementation_scaffolding {
        effective_prompt.push_str(templates::git_staging_guard());
    }

    if let Some(parent_id) = args.parent_task_id.as_deref()
        && let Some(parent) = store.get_task(parent_id)?
        && parent.status == TaskStatus::Done
        && let Some(summary_json) = store.get_completion_summary(parent_id)?
        && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&summary_json)
    {
        let summary_block = format_summary_for_injection(&summary);
        effective_prompt = format!("{summary_block}\n\n{effective_prompt}");
    }
    if let Some(ref group_id) = args.group
        && !matches!(agent_kind, AgentKind::OpenCode | AgentKind::Kilo | AgentKind::MiMoCode)
    {
        let sibling_summaries = prompt_context::collect_sibling_summaries(store, group_id, current_task_id)?;
        if !sibling_summaries.is_empty() {
            let block = sanitize_injected_text(&crate::cmd::summary::format_sibling_summaries(&sibling_summaries));
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }
    let prompt_skills = requested_skills
        .iter()
        .filter(|skill| !suppress_implementation_scaffolding || skill.as_str() != "implementer")
        .cloned()
        .collect::<Vec<_>>();
    let mut effective_prompt = inject_skill(
        &effective_prompt,
        agent_kind,
        &prompt_skills,
        !args.skills.is_empty(),
    )?;
    let mut injected_memory_ids = Vec::new();

    // Inject relevant memories from past tasks
    if let Some((memory_block, memory_ids)) = prompt_context::inject_memories(store, &args.prompt, 10, project_root.and_then(|root| root.to_str()))? {
        let memory_block = sanitize_injected_text(&memory_block);
        effective_prompt = format!("{memory_block}\n\n{effective_prompt}");
        injected_memory_ids = memory_ids;
    }

    let (mut effective_prompt, project_topics) = prompt_project::inject_project_context(
        effective_prompt, &args.prompt, detected_project, project_root,
    );

    // Inject team rules + knowledge if --team was specified
    if let Some(ref team_id) = args.team {
        if let Some(tc) = team::resolve_team(team_id)
            && !tc.rules.is_empty() {
                let rules_block = tc.rules.iter()
                    .map(|r| format!("- {r}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                effective_prompt = format!("<aid-team-rules>\n{rules_block}\n</aid-team-rules>\n\n{effective_prompt}");
                aid_info!("[aid] Injected {} team rule(s)", tc.rules.len());
            }
        let entries = team::read_knowledge_entries(team_id);
        let total_entries = entries.len();
        if total_entries > 0 {
            let relevant = prompt_context::select_relevant_entries(&entries, &args.prompt);
            let relevant: Vec<_> = if project_topics.is_empty() {
                relevant
            } else {
                relevant
                    .into_iter()
                    .filter(|entry| {
                        let entry_topic_words = prompt_context::extract_words(&entry.topic);
                        let overlap = entry_topic_words
                            .iter()
                            .filter(|word| project_topics.contains(*word))
                            .count();
                        let total = entry_topic_words.len().max(1);
                        (overlap as f64 / total as f64) < 0.5
                    })
                    .collect()
            };
            aid_info!("[aid] Injected {}/{} knowledge entries (relevance-filtered)", relevant.len(), total_entries);
            if !relevant.is_empty() {
                let knowledge_block = sanitize_injected_text(&prompt_context::format_knowledge_block(team_id, &relevant));
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
        }
    }

    // Inject team toolbox tools
    {
        let tools = toolbox::resolve_toolbox(
            args.team.as_deref(),
            project_root,
        );
        let tools = if let Some(ref team_id) = args.team {
            if let Some(tc) = team::resolve_team(team_id)
                && !tc.toolbox.auto_inject.is_empty()
            {
                toolbox::filter_by_auto_inject(tools, &tc.toolbox.auto_inject)
            } else {
                tools
            }
        } else {
            tools
        };
        let before_count = tools.len();
        // Narrowing happens only when the caller declared a kind. A guessed
        // category used to decide it, so a multi-file refactor described in one
        // sentence got 2 of 24 tools and never learned what it was missing.
        // Omission now means everything, because omission is not a decision.
        let (tools, filter_note) = match args.kind {
            Some(_) => (
                toolbox::filter_by_task_category(tools, task_category_label),
                format!("declared kind: {task_category_label}"),
            ),
            None => (tools, "no kind declared".to_string()),
        };
        aid_info!(
            "[aid] Injected {}/{} toolbox tool(s) ({})",
            tools.len(),
            before_count,
            filter_note
        );
        if !tools.is_empty() {
            let toolbox_block = toolbox::format_toolbox_instructions(&tools);
            effective_prompt = format!("{effective_prompt}\n\n{toolbox_block}");
        }
    }

    // Inject output from previous tasks (--context-from)
    if !args.context_from.is_empty()
        && let Some(block) = prompt_context::resolve_context_from(store, &args.context_from)?
    {
        let token_count = templates::estimate_tokens(&block);
        aid_info!("[aid] Injected context from {} task(s) (~{token_count} tokens)", args.context_from.len());
        effective_prompt = format!("{block}\n\n{effective_prompt}");
    }

    if let Ok(shared_dir) = std::env::var("AID_SHARED_DIR") {
        effective_prompt = format!(
            "[Shared Directory]\nA shared directory is available at: {shared_dir}\nWrite files here that other tasks in the batch need to read.\nRead files here that other tasks may have produced.\n\n{effective_prompt}"
        );
    }

    // Inject workspace path if workgroup has one (appended to avoid commit message pollution)
    if let Some(ref group_id) = args.group {
        let workspace = crate::paths::workspace_dir(group_id)?;
        if workspace.is_dir() {
            let workspace_ref = if agent_kind.sandboxed_fs() {
                ".aid-workspace".to_string()
            } else {
                workspace.display().to_string()
            };
            effective_prompt = format!(
                "{effective_prompt}\n\n<aid-system-context>\n[Shared Workspace] Path: {workspace_ref} — use for intermediate artifacts and inter-agent communication.\n</aid-system-context>",
            );
        }
    }

    if !args.batch_siblings.is_empty() {
        effective_prompt = format!(
            "{effective_prompt}\n\n{}",
            format_batch_siblings(&args.batch_siblings)
        );
    }
    if let Some(block) = output_file_instruction(args.output.as_deref(), args.result_file.as_deref()) {
        effective_prompt = format!("{effective_prompt}\n\n{block}");
    }
    if let Some(block) = crate::cmd::report_mode::instruction(
        &args.prompt, args.read_only, task_profile.category, args.result_file.as_deref(), args.kind,
    ) {
        effective_prompt = format!("{effective_prompt}\n\n{block}");
    }
    if let Some(checklist_block) = crate::cmd::checklist::format_checklist_block(&args.checklist) {
        effective_prompt = format!("{effective_prompt}\n\n{checklist_block}");
    }
    if let Some(line) = rust_cache_prompt_line(args.dir.as_deref()) {
        effective_prompt = format!("{line}\n\n{effective_prompt}");
    }

    // Compact prompt if it exceeds token budget
    let effective_prompt = maybe_compact_prompt(&effective_prompt, PROMPT_TOKEN_LIMIT);
    let prompt_tokens = templates::estimate_tokens(&effective_prompt) as i64;
    Ok(PromptBundle { effective_prompt, context_files, prompt_tokens, injected_memory_ids })
}

#[cfg(test)] #[path = "prompt_tests.rs"] mod tests;

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
        assert_eq!(effective_category(TaskCategory::Frontend, None), TaskCategory::Frontend);
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
