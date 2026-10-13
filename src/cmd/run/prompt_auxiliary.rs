// Bounded summaries, methodology, and prior output for prompt assembly.
// Collects omission descriptions for the parent builder's single warning.
use super::*;

pub(super) fn inject_related_context(
    store: &Store,
    args: &RunArgs,
    agent_kind: &AgentKind,
    current_task_id: &str,
    requested_skills: &[String],
    suppress_implementation_scaffolding: bool,
    effective_prompt: String,
    omissions: &mut Vec<String>,
) -> Result<String> {
    let prompt = inject_task_summaries(
        store,
        args,
        agent_kind,
        current_task_id,
        effective_prompt,
        omissions,
    )?;
    inject_methodology(
        args,
        agent_kind,
        requested_skills,
        suppress_implementation_scaffolding,
        &prompt,
        omissions,
    )
}

pub(super) fn inject_task_summaries(
    store: &Store,
    args: &RunArgs,
    agent_kind: &AgentKind,
    current_task_id: &str,
    mut effective_prompt: String,
    omissions: &mut Vec<String>,
) -> Result<String> {
    if let Some(parent_id) = args.parent_task_id.as_deref()
        && let Some(parent) = store.get_task(parent_id)?
        && parent.status == TaskStatus::Done
        && let Some(summary_json) = store.get_completion_summary(parent_id)?
        && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&summary_json)
    {
        let summary_block = crate::workgroup::bound_auxiliary(
            &format_summary_for_injection(&summary),
            "Parent Task Context",
            omissions,
        );
        effective_prompt = format!("{summary_block}\n\n{effective_prompt}");
    }
    if let Some(ref group_id) = args.group
        && !matches!(
            agent_kind,
            AgentKind::OpenCode | AgentKind::Kilo | AgentKind::MiMoCode
        )
    {
        let sibling_summaries =
            prompt_context::collect_sibling_summaries(store, group_id, current_task_id)?;
        if !sibling_summaries.is_empty() {
            let block = sanitize_injected_text(&crate::cmd::summary::format_sibling_summaries(
                &sibling_summaries,
            ));
            let block =
                crate::workgroup::bound_auxiliary(&block, "Sibling Task Context", omissions);
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }
    Ok(effective_prompt)
}

pub(super) fn inject_methodology(
    args: &RunArgs,
    agent_kind: &AgentKind,
    requested_skills: &[String],
    suppress_implementation_scaffolding: bool,
    effective_prompt: &str,
    omissions: &mut Vec<String>,
) -> Result<String> {
    let prompt_skills = requested_skills
        .iter()
        .filter(|skill| !suppress_implementation_scaffolding || skill.as_str() != "implementer")
        .cloned()
        .collect::<Vec<_>>();
    let effective_prompt = inject_skill(
        &effective_prompt,
        agent_kind,
        &prompt_skills,
        !args.skills.is_empty(),
        omissions,
    )?;
    Ok(effective_prompt)
}

pub(super) fn inject_prior_output(
    store: &Store,
    args: &RunArgs,
    mut effective_prompt: String,
    omissions: &mut Vec<String>,
) -> Result<String> {
    // Inject output from previous tasks (--context-from)
    if !args.context_from.is_empty()
        && let Some(block) =
            prompt_context::resolve_context_from(store, &args.context_from, omissions)?
    {
        let token_count = templates::estimate_tokens(&block);
        aid_info!(
            "[aid] Injected context from {} task(s) (~{token_count} tokens)",
            args.context_from.len()
        );
        effective_prompt = format!("{block}\n\n{effective_prompt}");
    }

    Ok(effective_prompt)
}
