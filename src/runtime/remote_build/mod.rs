// Remote Cargo dispatch: resolve a box, persist it, and configure task commands.
// Exports task-start resolution, shim installation, prompt and verify integration.
// Deps: RunArgs, Store, std process/filesystem, and the embedded Bash shim.
use anyhow::{Context, Result, bail};
use std::path::Path;
use std::process::Command;
use crate::cmd::run::RunArgs;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

pub(crate) fn resolve(args: &mut RunArgs) -> Result<()> {
    resolve_in(args, crate::agent::env::which_exists("rbox"), &mut Command::new("rbox"))
}

fn resolve_in(args: &mut RunArgs, rbox_present: bool, command: &mut Command) -> Result<()> {
    let Some(requested) = args.remote_build.as_deref() else { return Ok(()) };
    if args.sandbox || args.container.is_some() {
        bail!("--remote-build conflicts with --sandbox and --container: rbox needs host Tailscale");
    }
    if !rbox_present {
        bail!("Remote build requires rbox on PATH; rbox CLI not found");
    }
    let repo = match args.dir.as_deref() {
        Some(dir) => std::path::PathBuf::from(dir),
        None => std::env::current_dir().context("Failed to read the current directory")?,
    };
    let auto_picked = requested == "auto" || args.remote_build_auto_picked;
    args.remote_build = Some(resolve_with(requested, &repo, command)?);
    args.remote_build_auto_picked = auto_picked;
    Ok(())
}

/// Project-level default: `auto` degrades to a local build when rbox is absent;
/// a named box is kept and fails later in `resolve` exactly like an explicit flag.
pub(crate) fn project_default(value: Option<&str>) -> Option<String> {
    project_default_with(value, crate::agent::env::which_exists("rbox"))
}

fn project_default_with(value: Option<&str>, rbox_present: bool) -> Option<String> {
    match value {
        Some("auto") if !rbox_present => {
            aid_warn!("[aid] project remote_build=auto ignored: rbox not on PATH");
            None
        }
        other => other.map(str::to_string),
    }
}

fn resolve_with(requested: &str, repo: &Path, command: &mut Command) -> Result<String> {
    if requested != "auto" { return Ok(requested.to_string()); }
    pick(command, repo, None)
}

fn pick(command: &mut Command, start_dir: &Path, exclude: Option<&str>) -> Result<String> {
    let repo = crate::config::project::worktree::main_working_tree(start_dir)
        .unwrap_or_else(|| start_dir.to_path_buf());
    command.args(["pick", "--tool", "rust", "--repo"]).arg(repo);
    if let Some(excluded) = exclude { command.args(["--exclude", excluded]); }
    let output = command.output().context("Failed to run rbox pick --tool rust")?;
    if !output.status.success() {
        bail!("Remote build box selection failed: {}", String::from_utf8_lossy(&output.stderr).trim());
    }
    let name = String::from_utf8(output.stdout).context("rbox pick returned invalid UTF-8")?;
    let name = name.trim();
    if name.is_empty() { bail!("rbox pick returned an empty build box name"); }
    Ok(name.to_string())
}

pub(crate) fn saved_box(store: &Store, task_id: &str) -> Result<Option<String>> {
    Ok(RunArgs::saved_for_task(store, task_id)?.and_then(|args| args.remote_build))
}

pub(crate) fn record(store: &Store, task_id: &TaskId, args: &RunArgs) -> Result<()> {
    let Some(name) = args.remote_build.as_deref() else { return Ok(()) };
    store.insert_event(&TaskEvent {
        task_id: task_id.clone(), timestamp: chrono::Local::now(), event_kind: EventKind::Milestone,
        detail: format!("Remote build box: {name}"),
        metadata: Some(serde_json::json!({"remote_build": name})),
    })?;
    Ok(())
}

pub(crate) fn append_prompt(prompt: &mut String, name: Option<&str>) {
    if let Some(name) = name {
        prompt.push_str(&format!("\n\nCargo build/check/test run on remote box {name} through a PATH shim. Expect sync and lock latency; do not bypass the shim. `cargo fmt` stays local."));
    }
}

pub(crate) fn configure_task(
    store: &Store, task_id: &str, cmd: &mut Command, home: &Path,
) -> Result<()> {
    if let Some(name) = saved_box(store, task_id)? {
        configure(cmd, home, &name)?;
    }
    Ok(())
}

fn configure(cmd: &mut Command, home: &Path, name: &str) -> Result<()> {
    let shim_dir = home.join(".aid-build-shims");
    if std::fs::symlink_metadata(&shim_dir).is_ok_and(|m| m.file_type().is_symlink()) {
        bail!("Remote Cargo shim directory must not be a symlink: {}", shim_dir.display());
    }
    std::fs::create_dir_all(&shim_dir).context("Failed to create remote Cargo shim directory")?;
    let shim = shim_dir.join("cargo");
    std::fs::write(&shim, include_str!("cargo.sh"))?;
    #[cfg(unix)] {
        use std::os::unix::fs::PermissionsExt;
        std::fs::set_permissions(&shim, std::fs::Permissions::from_mode(0o755))?;
    }
    let path = cmd.get_envs().find(|(key, _)| *key == "PATH")
        .and_then(|(_, value)| value.map(ToOwned::to_owned))
        .or_else(|| std::env::var_os("PATH")).unwrap_or_default();
    let paths = std::iter::once(shim_dir).chain(std::env::split_paths(&path));
    cmd.env("PATH", std::env::join_paths(paths)?).env("AID_BUILD_BOX", name);
    Ok(())
}

mod lock;
pub(crate) use lock::LockQueue;

mod selection;

pub(crate) fn verify(
    store: &Store, task_id: &str, path: &Path, command: Option<&str>,
    target: Option<&str>, container: Option<&str>,
) -> Result<crate::task::verify::VerifyResult> {
    verify_with(store, task_id, path, command, target, container, &mut Command::new("rbox"))
}

fn verify_with(
    store: &Store, task_id: &str, path: &Path, command: Option<&str>,
    target: Option<&str>, container: Option<&str>, rbox: &mut Command,
) -> Result<crate::task::verify::VerifyResult> {
    let Some(name) = saved_box(store, task_id)? else {
        return crate::task::verify::cargo::run_verify_with_store(store, path, command, target, container);
    };
    let deadline = verify_deadline(store, task_id, chrono::Local::now())?;
    let result = verify_on(task_id, &name, path, command, target, container, deadline)?;
    let auto_picked = RunArgs::saved_for_task(store, task_id)?
        .is_some_and(|args| args.remote_build_auto_picked);
    if !result.infrastructure_failure || result.exit_code.is_none() || !auto_picked
        || !crate::task::verify::infrastructure::retryable(&result.output)
        || remaining_verify_time(deadline, chrono::Local::now()).is_zero()
    { return Ok(result); }
    let refusal = refusal_line(&result.output);
    let replacement = selection::pick_before(rbox, path, &name, deadline).map_err(|error| {
        anyhow::anyhow!("Remote build box {name}: {refusal}; re-pick failed: {error}")
    })?;
    store.insert_event(&TaskEvent {
        task_id: TaskId(task_id.to_string()), timestamp: chrono::Local::now(),
        event_kind: EventKind::Milestone,
        detail: format!(
            "Remote build box: {name} infrastructure failure ({refusal}); re-picked {replacement}"
        ),
        metadata: Some(serde_json::json!({"remote_build": replacement, "refused_box": name})),
    })?;
    persist_box(store, task_id, &replacement)?;
    verify_on(task_id, &replacement, path, command, target, container, deadline)
}

fn verify_on(
    task_id: &str, name: &str, path: &Path, command: Option<&str>,
    target: Option<&str>, container: Option<&str>, deadline: chrono::DateTime<chrono::Local>,
) -> Result<crate::task::verify::VerifyResult> {
    if remaining_verify_time(deadline, chrono::Local::now()).is_zero() {
        return Ok(crate::task::verify::VerifyBudget::no_launch(command.unwrap_or("auto").to_string()));
    }
    let mut environment = Command::new("cargo");
    configure(&mut environment, &crate::support::paths::task_dir(task_id).join("home"), name)?;
    let env = environment.get_envs().filter_map(|(key, value)| {
        value.map(|value| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
    }).collect::<Vec<_>>();
    let mut result = crate::task::verify::run_verify_with_env(
        path, command, target, container, crate::task::verify::VerifyBudget::Deadline(deadline), &env,
    )?;
    result.infrastructure_failure = !result.success && !result.timed_out
        && crate::task::verify::infrastructure::diagnostic(&result.output).is_some();
    Ok(result)
}

fn verify_deadline(
    store: &Store, task_id: &str, now: chrono::DateTime<chrono::Local>,
) -> Result<chrono::DateTime<chrono::Local>> {
    let args = RunArgs::saved_for_task(store, task_id)?
        .with_context(|| format!("Task {task_id} has no saved dispatch args for verification"))?;
    let (started_at, created_at): (Option<String>, String) = store.db().query_row(
        "SELECT started_at, created_at FROM tasks WHERE id = ?1", [task_id],
        |row| Ok((row.get(0)?, row.get(1)?)),
    )?;
    let start = chrono::DateTime::parse_from_rfc3339(started_at.as_deref().unwrap_or(&created_at))
        .context("Invalid task start timestamp for remote verification")?
        .with_timezone(&chrono::Local).min(now);
    let policy = args.timeout_policy;
    let allowance = chrono::Duration::from_std(policy.max_duration.min(policy.hard_cap))
        .context("Remote verification allowance is out of range")?;
    start.checked_add_signed(allowance).context("Remote verification deadline is out of range")
}

fn remaining_verify_time(
    deadline: chrono::DateTime<chrono::Local>, now: chrono::DateTime<chrono::Local>,
) -> std::time::Duration {
    deadline.signed_duration_since(now).to_std().unwrap_or_default()
}

fn refusal_line(output: &str) -> String {
    crate::task::verify::infrastructure::diagnostic(output)
        .unwrap_or("remote verification infrastructure failure").to_string()
}

fn persist_box(store: &Store, task_id: &str, name: &str) -> Result<()> {
    let mut args = RunArgs::saved_for_task(store, task_id)?
        .with_context(|| format!("Task {task_id} has no saved dispatch args to pin the build box in"))?;
    args.remote_build = Some(name.to_string());
    store.update_task_dispatch_args(task_id, &args.dispatch_args_json()?)
}

#[cfg(test)]
mod tests;
#[cfg(test)]
mod shim_tests;
#[cfg(test)]
mod deadline_tests;
#[cfg(test)]
mod launch_deadline_tests;

#[cfg(test)]
mod infrastructure_tests;
