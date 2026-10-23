// Worker command launch from saved RunArgs and transient runtime state.
// Exports prepare_launch; depends on agent isolation, wrappers and Store.
use super::BackgroundRunSpec;
use crate::cmd::run::RunArgs;
use crate::{agent, store::Store, types::TaskId};
use anyhow::Result;
use std::sync::Arc;

pub(super) fn prepare_launch(
    store: &Arc<Store>,
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    agent: &dyn agent::Agent,
    home_guard: &agent::home_isolation::IsolatedHomeGuard,
) -> Result<(std::process::Command, Option<String>)> {
    let opts = run_opts(spec, args, agent.kind());
    let task = store.get_task(&spec.task_id)?;
    let launch_prompt = task
        .as_ref()
        .and_then(|task| task.resolved_prompt.as_deref())
        .unwrap_or(&args.prompt);
    let worktree_branch = task.as_ref().and_then(|task| task.worktree_branch.clone());
    let mut cargo_target_dir =
        agent::rust_build_cache_target_dir(args.dir.as_deref(), worktree_branch.as_deref());
    let uses_durable_codex_home =
        agent::should_use_durable_codex_home(agent.kind(), args.sandbox, args.container.is_some());
    let (temp_dir, writable_roots) =
        prepare_scratch(store, spec, args, agent, home_guard, &mut cargo_target_dir)?;
    let mut std_cmd = build_launch_command(
        agent,
        launch_prompt,
        &opts,
        uses_durable_codex_home,
        writable_roots,
    )?;
    ensure_launch_binary(args, &std_cmd)?;
    agent::apply_run_env(&mut std_cmd, &opts, home_guard);
    crate::runtime::remote_build::configure_task(
        store,
        &spec.task_id,
        &mut std_cmd,
        home_guard.path(),
    )?;
    if let Some(temp_dir) = temp_dir {
        std_cmd.env("TMPDIR", temp_dir);
    }
    finalize_command_env(
        store,
        spec,
        args,
        agent,
        &mut std_cmd,
        cargo_target_dir.as_deref(),
    )?;
    wrap_command(spec, args, agent.kind(), std_cmd)
}

fn run_opts(
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    kind: crate::types::AgentKind,
) -> agent::RunOpts {
    let (output, result_file) = crate::cmd::run::read_only_delivery::agent_paths(args, kind);
    agent::RunOpts {
        dir: args.dir.clone(),
        output,
        result_file,
        model: args.model.clone(),
        budget: args.budget,
        read_only: args.read_only,
        sandbox: args.sandbox,
        context_files: vec![],
        session_id: args.session_id.clone(),
        env: agent::env_with_agent_log(
            args.env.clone(),
            &spec.task_id,
            args.container.is_none() && !args.sandbox,
        ),
        env_forward: args.env_forward.clone(),
    }
}

fn finish_env(
    store: &Store,
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    agent_kind: crate::types::AgentKind,
    cmd: &mut std::process::Command,
    cargo_target_dir: Option<&str>,
) -> Result<()> {
    if agent::should_use_durable_codex_home(agent_kind, args.sandbox, args.container.is_some()) {
        if args
            .session_id
            .as_deref()
            .is_some_and(agent::codex::resume_fallback_needed)
        {
            store.insert_event(&agent::codex::resume_fallback_event(&TaskId(
                spec.task_id.clone(),
            )))?;
        }
        agent::apply_codex_home_env(cmd)?;
    }
    if let Some(ref dir) = args.dir {
        agent::set_git_ceiling(cmd, dir);
    }
    if let Some(ref group) = args.group {
        cmd.env("AID_GROUP", group);
    }
    cmd.env("AID_TASK_ID", &spec.task_id);
    let depth = crate::cmd::run::task_depth(store, &spec.task_id).unwrap_or(0);
    cmd.env("AID_TASK_DEPTH", depth.to_string());
    agent::apply_cargo_target_env(cmd, cargo_target_dir);
    Ok(())
}

fn wrap_command(
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    agent_kind: crate::types::AgentKind,
    cmd: std::process::Command,
) -> Result<(std::process::Command, Option<String>)> {
    let container_name = if let Some(image) = args.container.as_deref() {
        let project_dir = args
            .dir
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("."));
        let project_id = crate::config::project::detect_project_in(project_dir)
            .map(|project| project.id)
            .unwrap_or_else(|| spec.task_id.clone());
        Some(crate::runtime::container::start_or_reuse(
            image,
            project_dir,
            &project_id,
        )?)
    } else {
        None
    };
    let cmd = if let Some(container_name) = container_name.as_deref() {
        crate::runtime::container::exec_in_container(&cmd, container_name)
    } else if args.sandbox && crate::runtime::sandbox::can_sandbox(agent_kind) {
        if !crate::runtime::sandbox::is_available() {
            anyhow::bail!("--sandbox requires container CLI");
        }
        crate::runtime::sandbox::wrap_command(&cmd, &spec.task_id, agent_kind, args.read_only)
    } else {
        cmd
    };
    Ok((cmd, container_name))
}

#[cfg(test)]
#[path = "launch_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "launch_api_route_tests.rs"]
mod api_route_tests;

fn prepare_scratch(
    store: &Store,
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    agent: &dyn agent::Agent,
    home_guard: &agent::home_isolation::IsolatedHomeGuard,
    cargo_target_dir: &mut Option<String>,
) -> Result<(Option<std::path::PathBuf>, Vec<std::path::PathBuf>)> {
    let mut temp_dir = None;
    let mut writable_roots = Vec::new();
    if args.container.is_none() && !args.sandbox {
        let temp = if crate::cmd::run::read_only_delivery::enabled(args, agent.kind()) {
            std::path::PathBuf::from(
                args.artifact_staging
                    .as_deref()
                    .ok_or_else(|| anyhow::anyhow!("Read-only artifact staging is unavailable"))?,
            )
        } else {
            *cargo_target_dir = agent::scratch::prepare_cargo_target(cargo_target_dir.as_deref())?;
            agent::scratch::create_temp_dir(home_guard.path())?
        };
        let (roots, warnings) = agent::scratch::prepare_launch_roots(
            agent.kind(),
            args.dir.as_deref(),
            cargo_target_dir.as_deref(),
            &temp,
            &TaskId(spec.task_id.clone()),
            args.read_only,
        )?;
        for warning in warnings {
            store.insert_event(&warning)?;
        }
        temp_dir = Some(temp);
        writable_roots = roots;
    }
    Ok((temp_dir, writable_roots))
}

fn finalize_command_env(
    store: &Store,
    spec: &BackgroundRunSpec,
    args: &RunArgs,
    agent: &dyn agent::Agent,
    std_cmd: &mut std::process::Command,
    cargo_target_dir: Option<&str>,
) -> Result<()> {
    finish_env(store, spec, args, agent.kind(), std_cmd, cargo_target_dir)?;
    crate::agent::api_route::validate_launch(agent, store, args.container.is_some())?;
    agent.finalize_launch_env(std_cmd);
    if let Some(route) = agent.api_route() {
        store.insert_agent_event(&crate::types::TaskEvent {
            task_id: TaskId(spec.task_id.clone()),
            timestamp: chrono::Local::now(),
            event_kind: crate::types::EventKind::Setup,
            detail: format!("API route {} launch", route.id),
            metadata: Some(serde_json::json!({"api_route_launch": route.id})),
        })?;
    }
    Ok(())
}

fn build_launch_command(
    agent: &dyn agent::Agent,
    launch_prompt: &str,
    opts: &agent::RunOpts,
    uses_durable_codex_home: bool,
    writable_roots: Vec<std::path::PathBuf>,
) -> Result<std::process::Command> {
    let std_cmd = agent
        .build_command_with_context(
            launch_prompt,
            &opts,
            agent::CommandContext {
                durable_codex_home: uses_durable_codex_home,
                writable_roots,
            },
        )
        .map_err(|err| anyhow::anyhow!("Failed to build agent command: {err:#}"))?;
    Ok(std_cmd)
}

fn ensure_launch_binary(args: &RunArgs, std_cmd: &std::process::Command) -> Result<()> {
    if args.container.is_none() && !args.sandbox {
        let program = std_cmd.get_program().to_string_lossy();
        agent::ensure_resolved_binary_available(&args.agent_name, &program)?;
    }
    Ok(())
}
