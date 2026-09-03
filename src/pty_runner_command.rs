// PTY command extraction and spawn-error logging.
// Exports spawn_bridge for pty_runner.
// Deps: PtyBridge, std::process::Command, and filesystem paths.

use anyhow::Result;
use std::path::Path;

use crate::pty_bridge::PtyBridge;

pub(crate) fn spawn_bridge(
    cmd: &std::process::Command,
    log_path: &Path,
) -> Result<PtyBridge> {
    let (argv, dir, env) = command_parts(cmd);
    match PtyBridge::spawn(&argv, dir.as_deref(), env) {
        Ok(bridge) => Ok(bridge),
        Err(err) => {
            let error_msg = format!("Failed to spawn agent process: {err}");
            aid_error!("[aid] {error_msg}");
            write_spawn_error_log(log_path, &error_msg);
            Err(anyhow::anyhow!(error_msg))
        }
    }
}

fn command_parts(
    cmd: &std::process::Command,
) -> (Vec<String>, Option<String>, Vec<(String, String)>) {
    let argv = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let dir = cmd
        .get_current_dir()
        .map(|path| path.to_string_lossy().into_owned());
    let env = cmd
        .get_envs()
        .filter_map(|(key, value)| {
            Some((
                key.to_string_lossy().into_owned(),
                value?.to_string_lossy().into_owned(),
            ))
        })
        .collect();
    (argv, dir, env)
}

fn write_spawn_error_log(log_path: &Path, message: &str) {
    let event = serde_json::json!({
        "type": "error",
        "source": "spawn",
        "message": message,
        "timestamp": chrono::Local::now().to_rfc3339(),
    });
    let _ = std::fs::write(log_path, format!("{event}\n"));
}
