// Pre-dispatch validation and task ID conflict handling for `aid run`.
// Exports: validate_dispatch(), resolve_id_conflict(), IdConflict.
// Deps: agent classification, Store, RunArgs, task status types.
use anyhow::Result;
use crate::agent;
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use super::RunArgs;

pub(super) fn validate_dispatch(args: &RunArgs, agent_kind: &AgentKind) -> Vec<String> {
    let mut warnings = Vec::new();
    let prompt_len = args.prompt.chars().count();
    if prompt_len < 10 {
        warnings.push("Prompt is very short, agent may not have enough context".to_string());
    }
    if matches!(
        agent_kind,
        AgentKind::Codex
            | AgentKind::Claude
            | AgentKind::OpenCode
            | AgentKind::Cursor
            | AgentKind::Kilo
            | AgentKind::Codebuff
    ) && args.dir.is_none() && !args.read_only
    {
        let profile = agent::classifier::classify(
            &args.prompt,
            agent::classifier::count_file_mentions(&args.prompt),
            prompt_len,
        );
        if !matches!(
            profile.category,
            agent::classifier::TaskCategory::Research | agent::classifier::TaskCategory::Documentation
        ) {
            warnings.push("Code agent without --dir may not be able to write files".to_string());
        }
    }
    if prompt_len > 5000 {
        warnings.push(format!(
            "Very long prompt ({prompt_len} chars), consider using --context files instead"
        ));
    }
    if matches!(agent_kind, AgentKind::Gemini) && args.worktree.is_some() {
        warnings.push("Research agent with --worktree is unusual, did you mean a code agent?".to_string());
    }
    warnings
}

pub(super) enum IdConflict {
    None,
    ReplaceWaiting,
    Running,
    AutoSuffix(String),
}

pub(super) fn resolve_id_conflict(store: &Store, id: &str) -> Result<IdConflict> {
    let Some(existing) = store.get_task(id)? else {
        return Ok(IdConflict::None);
    };
    match existing.status {
        TaskStatus::Waiting => Ok(IdConflict::ReplaceWaiting),
        TaskStatus::Running => Ok(IdConflict::Running),
        _ => {
            for suffix in 2..=99 {
                let candidate = format!("{id}-{suffix}");
                if store.get_task(&candidate)?.is_none() {
                    return Ok(IdConflict::AutoSuffix(candidate));
                }
            }
            anyhow::bail!("Too many tasks with ID prefix '{id}' (checked up to -99)");
        }
    }
}
