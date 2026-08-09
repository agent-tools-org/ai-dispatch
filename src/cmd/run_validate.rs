// Pre-dispatch validation and task ID conflict handling for `aid run`.
// Exports: validate_dispatch(), validate_command_preflight(), resolve_id_conflict(), IdConflict.
// Deps: agent classification, Store, RunArgs, task status types.
use anyhow::Result;
use crate::agent::{self, RunOpts};
use crate::store::Store;
use crate::types::{AgentKind, TaskStatus};
use super::RunArgs;

/// Reject combinations the command builder cannot honor before a task row exists.
pub(super) fn validate_command_preflight(
    agent: &dyn agent::Agent,
    args: &RunArgs,
    effective_model: Option<&str>,
) -> Result<()> {
    validate_command_preflight_with(agent, args, effective_model, crate::agent::env::which_exists)
}

pub(super) fn validate_command_preflight_with<F>(
    agent: &dyn agent::Agent,
    args: &RunArgs,
    effective_model: Option<&str>,
    which: F,
) -> Result<()>
where
    F: Fn(&str) -> bool,
{
    if args.sandbox
        && crate::sandbox::can_sandbox(agent.kind())
        && !crate::sandbox::is_available()
    {
        anyhow::bail!(
            "--sandbox requires Apple Container CLI. Install: brew install container; or omit --sandbox"
        );
    }
    let opts = RunOpts {
        // Capability probe only — skip dir existence checks (worktree may not exist yet).
        dir: None,
        output: args.output.clone(),
        result_file: args.result_file.clone(),
        model: effective_model.map(str::to_string),
        budget: args.budget,
        read_only: args.read_only,
        sandbox: args.sandbox,
        context_files: Vec::new(),
        session_id: None,
        env: None,
        env_forward: None,
    };
    let cmd = agent
        .build_command("__aid_preflight__", &opts)
        .map_err(|err| anyhow::anyhow!("{err:#}"))?;
    // Container/sandbox resolve the binary in the guest; dry-run never spawns.
    if args.container.is_some() || args.sandbox || args.dry_run {
        return Ok(());
    }
    let program = cmd.get_program().to_string_lossy();
    agent::ensure_resolved_binary_available_with(&args.agent_name, &program, which)?;
    agent.validate_cli()
}

pub(super) fn validate_dispatch(args: &RunArgs, agent_kind: &AgentKind) -> Vec<String> {
    let mut warnings = Vec::new();
    let prompt_len = args.prompt.chars().count();
    if prompt_len < 10 {
        warnings.push("Prompt is very short, agent may not have enough context".to_string());
    }
    if matches!(
        agent_kind,
        AgentKind::Codex
            | AgentKind::Copilot
            | AgentKind::Claude
            | AgentKind::OpenCode
            | AgentKind::CommandCode
            | AgentKind::Cursor
            | AgentKind::Qwen
            | AgentKind::Kilo
            | AgentKind::MiMoCode
            | AgentKind::Grok
    ) && args.dir.is_none() && args.worktree.is_none() && !args.read_only
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
