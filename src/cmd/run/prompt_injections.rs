// Auxiliary prompt injections and delivery instructions.
// Uses the parent prompt builder's context helpers and resolved dispatch inputs.
use super::*;

pub(super) fn inject_team_context(
    args: &RunArgs,
    mut effective_prompt: String,
    project_topics: &std::collections::HashSet<String>,
    omissions: &mut Vec<String>,
) -> String {
    // Inject team rules + knowledge if --team was specified
    if let Some(ref team_id) = args.team {
        if let Some(tc) = team::resolve_team(team_id)
            && !tc.rules.is_empty()
        {
            let rules_block = tc
                .rules
                .iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n");
            let rules_block =
                crate::task::workgroup::bound_auxiliary(&rules_block, "Team Rules", omissions);
            effective_prompt =
                format!("<aid-team-rules>\n{rules_block}\n</aid-team-rules>\n\n{effective_prompt}");
            aid_info!("[aid] Injected {} team rule(s)", tc.rules.len());
        }
        let entries = team::read_knowledge_entries(team_id);
        let total_entries = entries.len();
        if total_entries > 0 {
            let (relevant, omitted) = prompt_context::select_relevant_entries(&entries, &args.prompt);
            let relevant = deduplicate_knowledge(relevant, project_topics);
            aid_info!(
                "[aid] Injected {}/{} knowledge entries (relevance-filtered)",
                relevant.len(),
                total_entries
            );
            if !relevant.is_empty() || omitted > 0 {
                let knowledge_block = sanitize_injected_text(
                    &prompt_context::format_knowledge_block(team_id, &relevant, omitted, omissions),
                );
                let knowledge_block = crate::task::workgroup::bound_auxiliary(
                    &knowledge_block,
                    "Team Knowledge",
                    omissions,
                );
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
        }
    }

    effective_prompt
}

pub(super) fn inject_toolbox(
    args: &RunArgs,
    project_root: Option<&std::path::Path>,
    task_category_label: &str,
    mut effective_prompt: String,
    omissions: &mut Vec<String>,
) -> String {
    // Inject team toolbox tools
    let tools = toolbox::resolve_toolbox(args.team.as_deref(), project_root);
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
        let toolbox_block = crate::task::workgroup::bound_auxiliary(
            &toolbox::format_toolbox_instructions(&tools), "Toolbox", omissions,
        );
        effective_prompt = format!("{effective_prompt}\n\n{toolbox_block}");
    }
    effective_prompt
}

pub(super) fn inject_workspace(
    args: &RunArgs,
    agent_kind: &AgentKind,
    mut effective_prompt: String,
    omissions: &mut Vec<String>,
) -> Result<String> {
    if let Ok(shared_dir) = std::env::var("AID_SHARED_DIR") {
        let block = format!(
            concat!(
                "[Shared Directory]\nA shared directory is available at: {shared_dir}\n",
                "Write files here that other tasks in the batch need to read.\n",
                "Read files here that other tasks may have produced."
            ),
            shared_dir = shared_dir
        );
        let block = crate::task::workgroup::bound_auxiliary(&block, "Shared Directory", omissions);
        effective_prompt = format!("{block}\n\n{effective_prompt}");
    }

    // Inject workspace path if workgroup has one (appended to avoid commit message pollution)
    if let Some(ref group_id) = args.group {
        let workspace = crate::support::paths::workspace_dir(group_id)?;
        if workspace.is_dir() {
            let workspace_ref = if agent_kind.sandboxed_fs() {
                ".aid-workspace".to_string()
            } else {
                workspace.display().to_string()
            };
            let block = format!(
                concat!(
                    "<aid-system-context>\n[Shared Workspace] Path: {workspace_ref} — ",
                    "use for intermediate artifacts and inter-agent communication.\n</aid-system-context>"
                ),
                workspace_ref = workspace_ref
            );
            let block = crate::task::workgroup::bound_auxiliary(&block, "Shared Workspace", omissions);
            effective_prompt = format!("{effective_prompt}\n\n{block}");
        }
    }

    Ok(effective_prompt)
}

pub(super) fn inject_delivery_instructions(
    args: &RunArgs,
    agent_kind: &AgentKind,
    category: agent::classifier::TaskCategory,
    mut effective_prompt: String,
    omissions: &mut Vec<String>,
) -> String {
    if !args.batch_siblings.is_empty() {
        effective_prompt = format!(
            "{effective_prompt}\n\n{}",
            format_batch_siblings(&args.batch_siblings, omissions)
        );
    }
    let (output, result_file) = super::super::read_only_delivery::agent_paths(args, *agent_kind);
    if let Some(block) =
        output_file_instruction(output.as_deref(), result_file.as_deref())
    {
        effective_prompt = format!("{effective_prompt}\n\n{block}");
    }
    if let Some(block) = crate::cmd::report_mode::instruction(
        &args.prompt,
        args.read_only,
        category,
        result_file.as_deref(),
        args.kind,
    ) {
        effective_prompt = format!("{effective_prompt}\n\n{block}");
    }
    if let Some(checklist_block) = crate::cmd::checklist::format_checklist_block(&args.checklist) {
        effective_prompt = format!("{effective_prompt}\n\n{checklist_block}");
    }
    if let Some(line) = rust_cache_prompt_line(args.dir.as_deref()) {
        effective_prompt = format!("{line}\n\n{effective_prompt}");
    }

    effective_prompt
}

pub(super) fn inject_stored_context(
    store: &Store,
    args: &RunArgs,
    project_root: Option<&std::path::Path>,
    detected_project: Option<&project::ProjectConfig>,
    category: agent::classifier::TaskCategory,
    effective_prompt: &mut String,
    omissions: &mut Vec<String>,
) -> Result<Vec<String>> {
    let mut injected_memory_ids = Vec::new();
    if let Some((memory_block, memory_ids)) = prompt_context::inject_memories(
        store,
        &args.prompt,
        10,
        project_root.and_then(|root| root.to_str()),
        omissions,
    )? {
        let block = sanitize_injected_text(&memory_block);
        let block = crate::task::workgroup::bound_auxiliary(&block, "Memory", omissions);
        *effective_prompt = format!("{block}\n\n{effective_prompt}");
        injected_memory_ids = memory_ids;
    }
    let (prompt, topics) = prompt_project::inject_project_context(
        std::mem::take(effective_prompt),
        &args.prompt,
        detected_project,
        project_root,
        omissions,
    );
    let prompt = inject_team_context(args, prompt, &topics, omissions);
    let prompt = inject_toolbox(args, project_root, category.label(), prompt, omissions);
    *effective_prompt = inject_prior_output(store, args, prompt, omissions)?;
    Ok(injected_memory_ids)
}

fn deduplicate_knowledge<'a>(
    relevant: Vec<&'a team::KnowledgeEntry>,
    project_topics: &std::collections::HashSet<String>,
) -> Vec<&'a team::KnowledgeEntry> {
    if project_topics.is_empty() {
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
    }
}

pub(super) fn inject_delivery_context(
    args: &RunArgs,
    agent_kind: &AgentKind,
    category: agent::classifier::TaskCategory,
    prompt: String,
    omissions: &mut Vec<String>,
) -> Result<String> {
    let prompt = inject_workspace(args, agent_kind, prompt, omissions)?;
    Ok(inject_delivery_instructions(
        args, agent_kind, category, prompt, omissions,
    ))
}
