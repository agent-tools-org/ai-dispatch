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
    args.remote_build = Some(resolve_with(requested, command)?);
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

fn resolve_with(requested: &str, command: &mut Command) -> Result<String> {
    if requested != "auto" { return Ok(requested.to_string()); }
    let output = command.args(["pick", "--role", "rust-build"]).output()
        .context("Failed to run rbox pick --role rust-build")?;
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
    std::fs::write(&shim, include_str!("remote_build/cargo.sh"))?;
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

pub(crate) fn verify(
    store: &Store, task_id: &str, path: &Path, command: Option<&str>,
    target: Option<&str>, container: Option<&str>,
) -> Result<crate::verify::VerifyResult> {
    let Some(name) = saved_box(store, task_id)? else {
        return crate::verify_cargo::run_verify_with_store(store, path, command, target, container);
    };
    let mut environment = Command::new("cargo");
    configure(&mut environment, &crate::paths::task_dir(task_id).join("home"), &name)?;
    let env = environment.get_envs().filter_map(|(key, value)| {
        value.map(|value| (key.to_string_lossy().into_owned(), value.to_string_lossy().into_owned()))
    }).collect::<Vec<_>>();
    crate::verify::run_verify_with_env(
        path, command, target, container, crate::verify::VERIFY_TIMEOUT, &env,
    )
}

#[cfg(test)]
#[path = "remote_build/tests.rs"]
mod tests;
#[cfg(test)]
#[path = "remote_build/shim_tests.rs"]
mod shim_tests;
