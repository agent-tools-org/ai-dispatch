// Worker command launch from saved RunArgs and transient runtime state.
// Exports prepare_launch; depends on agent isolation, wrappers and Store.
use anyhow::Result;
use std::sync::Arc;
use crate::{agent, store::Store, types::TaskId};
use crate::cmd::run::RunArgs;
use super::BackgroundRunSpec;

pub(super) fn prepare_launch(
    store: &Arc<Store>, spec: &BackgroundRunSpec, args: &RunArgs,
    agent: &dyn agent::Agent, home_guard: &agent::home_isolation::IsolatedHomeGuard,
) -> Result<(std::process::Command, Option<String>)> {
    let opts = run_opts(spec, args);
    let worktree_branch = store.get_task(&spec.task_id)?.and_then(|task| task.worktree_branch);
    let mut cargo_target_dir = agent::rust_build_cache_target_dir(args.dir.as_deref(), worktree_branch.as_deref());
    let uses_durable_codex_home = agent::should_use_durable_codex_home(agent.kind(), args.sandbox, args.container.is_some());
    let mut temp_dir = None;
    let mut writable_roots = Vec::new();
    if args.container.is_none() && !args.sandbox {
        cargo_target_dir = agent::scratch::prepare_cargo_target(cargo_target_dir.as_deref())?;
        let temp = agent::scratch::create_temp_dir(home_guard.path())?;
        let (roots, warnings) = agent::scratch::prepare_launch_roots(
            agent.kind(), args.dir.as_deref(), cargo_target_dir.as_deref(), &temp, &TaskId(spec.task_id.clone()),
        )?;
        for warning in warnings {
            store.insert_event(&warning)?;
        }
        temp_dir = Some(temp);
        writable_roots = roots;
    }
    let mut std_cmd = agent
        .build_command_with_context(
            &args.prompt,
            &opts,
            agent::CommandContext { durable_codex_home: uses_durable_codex_home, writable_roots },
        )
        .map_err(|err| anyhow::anyhow!("Failed to build agent command: {err:#}"))?;
    if args.container.is_none() && !args.sandbox {
        let program = std_cmd.get_program().to_string_lossy();
        agent::ensure_resolved_binary_available(&args.agent_name, &program)?;
    }
    agent::apply_run_env(&mut std_cmd, &opts, home_guard);
    crate::remote_build::configure_task(store, &spec.task_id, &mut std_cmd, home_guard.path())?;
    if let Some(temp_dir) = temp_dir { std_cmd.env("TMPDIR", temp_dir); }
    finish_env(store, spec, args, agent.kind(), &mut std_cmd, cargo_target_dir.as_deref())?;
    wrap_command(spec, args, agent.kind(), std_cmd)
}

fn run_opts(spec: &BackgroundRunSpec, args: &RunArgs) -> agent::RunOpts {
    agent::RunOpts {
        dir: args.dir.clone(),
        output: args.output.clone(),
        result_file: args.result_file.clone(),
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
    store: &Store, spec: &BackgroundRunSpec, args: &RunArgs,
    agent_kind: crate::types::AgentKind, cmd: &mut std::process::Command,
    cargo_target_dir: Option<&str>,
) -> Result<()> {
    if agent::should_use_durable_codex_home(agent_kind, args.sandbox, args.container.is_some()) {
        if args.session_id.as_deref().is_some_and(agent::codex::resume_fallback_needed) {
            store.insert_event(&agent::codex::resume_fallback_event(&TaskId(spec.task_id.clone())))?;
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
    spec: &BackgroundRunSpec, args: &RunArgs, agent_kind: crate::types::AgentKind,
    cmd: std::process::Command,
) -> Result<(std::process::Command, Option<String>)> {
    let container_name = if let Some(image) = args.container.as_deref() {
        let project_dir = args
            .dir
            .as_deref()
            .map(std::path::Path::new)
            .unwrap_or_else(|| std::path::Path::new("."));
        let project_id = crate::project::detect_project_in(project_dir)
            .map(|project| project.id)
            .unwrap_or_else(|| spec.task_id.clone());
        Some(crate::container::start_or_reuse(image, project_dir, &project_id)?)
    } else {
        None
    };
    let cmd = if let Some(container_name) = container_name.as_deref() {
        crate::container::exec_in_container(&cmd, container_name)
    } else if args.sandbox && crate::sandbox::can_sandbox(agent_kind) {
        if !crate::sandbox::is_available() {
            anyhow::bail!("--sandbox requires container CLI");
        }
        crate::sandbox::wrap_command(&cmd, &spec.task_id, agent_kind, args.read_only)
    } else {
        cmd
    };
    Ok((cmd, container_name))
}
