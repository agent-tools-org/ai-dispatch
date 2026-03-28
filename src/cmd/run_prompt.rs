// Prompt and run helpers for `aid run` — bundle, resolve, context, process runner, store.
use anyhow::Result;
use serde_json;
use std::collections::HashSet;
use crate::{agent, project, store::Store, templates, team, toolbox, types::*};
use crate::cmd::summary::{format_summary_for_injection, CompletionSummary};
mod prompt_context;
#[path = "run_output.rs"]
mod run_output;
#[path = "run_verify.rs"]
mod run_verify;
#[path = "run_scope.rs"]
mod run_scope;
pub(super) use run_output::{fill_empty_output_from_log, clean_output_if_jsonl, output_file_instruction, persist_result_file};
pub(super) use run_scope::warn_agent_committed_files_outside_scope;
pub(super) use run_verify::{maybe_auto_retry_after_checklist_miss_impl, maybe_auto_retry_after_verify_failure_impl, maybe_cleanup_fast_fail_impl, maybe_verify_impl};
#[path = "run_process.rs"]
mod run_process;
#[path = "run_prompt_helpers.rs"]
mod run_prompt_helpers;
pub(super) use run_process::*;
pub(super) use run_prompt_helpers::*;
use super::RunArgs;

const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";
const PROMPT_TOKEN_LIMIT: usize = 30_000;
const BATCH_SIBLING_LIMIT: usize = 10;
const BATCH_SIBLING_PROMPT_LIMIT: usize = 80;

pub(super) struct PromptBundle { pub effective_prompt: String, pub context_files: Vec<String>, pub prompt_tokens: i64, pub injected_memory_ids: Vec<String> }

fn sanitize_injected_text(text: &str) -> String {
    let mut result = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<aid-") && !trimmed.starts_with("</aid-") {
            inside = true;
            continue;
        }
        if trimmed.starts_with("</aid-") {
            inside = false;
            continue;
        }
        if !inside {
            result.push(line);
        }
    }
    result.join("\n")
}

fn truncate_batch_sibling_prompt(prompt: &str) -> String {
    let mut preview: String = prompt.chars().take(BATCH_SIBLING_PROMPT_LIMIT).collect();
    if prompt.chars().count() > BATCH_SIBLING_PROMPT_LIMIT {
        preview.push_str("...");
    }
    preview
}

pub(super) fn format_batch_siblings(siblings: &[(String, String, String)]) -> String {
    let shown = siblings
        .iter()
        .take(BATCH_SIBLING_LIMIT)
        .map(|(name, agent, prompt)| {
            format!(
                "- \"{}\" ({}): {}",
                name,
                agent,
                truncate_batch_sibling_prompt(prompt)
            )
        })
        .collect::<Vec<_>>();
    let remaining = siblings.len().saturating_sub(BATCH_SIBLING_LIMIT);
    let mut lines = vec![
        "<aid-batch-siblings>".to_string(),
        "Other tasks running in this batch:".to_string(),
    ];
    lines.extend(shown);
    if remaining > 0 {
        lines.push(format!("+ {remaining} more"));
    }
    lines.push("</aid-batch-siblings>".to_string());
    lines.join("\n")
}

pub(super) fn build_prompt_bundle(store: &Store, args: &RunArgs, agent_kind: &AgentKind, workgroup: Option<&Workgroup>, requested_skills: &[String], current_task_id: &str) -> Result<PromptBundle> {
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
    let task_profile = agent::classifier::classify(
        &prompt,
        agent::classifier::count_file_mentions(&prompt),
        prompt.len(),
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
    if !args.read_only {
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
        && !matches!(agent_kind, AgentKind::OpenCode | AgentKind::Kilo)
    {
        let sibling_summaries = prompt_context::collect_sibling_summaries(store, group_id, current_task_id)?;
        if !sibling_summaries.is_empty() {
            let block = sanitize_injected_text(&crate::cmd::summary::format_sibling_summaries(&sibling_summaries));
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }
    let mut effective_prompt = inject_skill(&effective_prompt, agent_kind, requested_skills, prompt.len())?;
    let mut injected_memory_ids = Vec::new();

    // Inject relevant memories from past tasks
    if let Some((memory_block, memory_ids)) = prompt_context::inject_memories(store, &args.prompt, 10)? {
        let memory_block = sanitize_injected_text(&memory_block);
        effective_prompt = format!("{memory_block}\n\n{effective_prompt}");
        injected_memory_ids = memory_ids;
    }

    let mut project_topics: HashSet<String> = HashSet::new();

    // Inject project rules + knowledge if a project was detected
    if let Some(pc) = project::detect_project() {
        let rules_count = pc.rules.len();
        if !pc.rules.is_empty() {
            let rules_block = pc.rules.iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n");
            effective_prompt = format!("<aid-project-rules>\n{rules_block}\n</aid-project-rules>\n\n{effective_prompt}");
        }
        if let Some(state_block) = prompt_context::inject_project_state() {
            let state_block = sanitize_injected_text(&state_block);
            effective_prompt = format!("{state_block}\n\n{effective_prompt}");
            aid_info!("[aid] Injected project state");
        }
        let knowledge_entries = prompt_context::detect_project_path()
            .map(|path| project::read_project_knowledge(std::path::Path::new(&path)))
            .unwrap_or_default();
        let total_knowledge = knowledge_entries.len();
        if total_knowledge > 0 {
            let relevant = prompt_context::select_relevant_entries(&knowledge_entries, &args.prompt);
            if !relevant.is_empty() {
                for entry in &relevant {
                    project_topics.extend(prompt_context::extract_words(&entry.topic));
                }
                let knowledge_block = sanitize_injected_text(&prompt_context::format_knowledge_block(&pc.id, &relevant));
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
            aid_info!("[aid] Project '{}' detected: {} rule(s), {}/{} knowledge entries", pc.id, rules_count, relevant.len(), total_knowledge);
        } else if rules_count > 0 {
            aid_info!("[aid] Project '{}' detected: {} rule(s)", pc.id, rules_count);
        }
    }

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
        let project_dir = prompt_context::detect_project_path().map(std::path::PathBuf::from);
        let tools = toolbox::resolve_toolbox(
            args.team.as_deref(),
            project_dir.as_deref(),
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
        let tools = toolbox::filter_by_task_category(tools, task_category_label);
        aid_info!(
            "[aid] Injected {}/{} toolbox tool(s) (filtered by {})",
            tools.len(),
            before_count,
            task_category_label
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
            effective_prompt = format!(
                "{effective_prompt}\n\n<aid-system-context>\n[Shared Workspace] Path: {} — use for intermediate artifacts and inter-agent communication.\n</aid-system-context>",
                workspace.display()
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
    if let Some(checklist_block) = crate::cmd::checklist::format_checklist_block(&args.checklist) {
        effective_prompt = format!("{effective_prompt}\n\n{checklist_block}");
    }

    // Compact prompt if it exceeds token budget
    let effective_prompt = maybe_compact_prompt(&effective_prompt, PROMPT_TOKEN_LIMIT);
    let prompt_tokens = templates::estimate_tokens(&effective_prompt) as i64;
    Ok(PromptBundle { effective_prompt, context_files, prompt_tokens, injected_memory_ids })
}

#[cfg(test)] #[path = "run_prompt_tests.rs"] mod tests;
