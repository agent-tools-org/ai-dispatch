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
) -> (Vec<String>, Option<String>, Vec<(String, Option<String>)>) {
    let argv = std::iter::once(cmd.get_program())
        .chain(cmd.get_args())
        .map(|value| value.to_string_lossy().into_owned())
        .collect();
    let dir = cmd
        .get_current_dir()
        .map(|path| path.to_string_lossy().into_owned());
    // `None` is a removal (`Command::env_remove`); the PTY spawn starts from
    // this process's environment, so a removal must travel as one.
    let env = cmd
        .get_envs()
        .map(|(key, value)| {
            (
                key.to_string_lossy().into_owned(),
                value.map(|value| value.to_string_lossy().into_owned()),
            )
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

#[cfg(test)]
mod tests {
    use super::command_parts;

    #[test]
    fn command_parts_keep_env_removals() {
        let mut cmd = std::process::Command::new("true");
        cmd.env("KEPT", "1").env_remove("CLAUDE_CONFIG_DIR");
        let (_, _, env) = command_parts(&cmd);
        assert!(env.contains(&("KEPT".to_string(), Some("1".to_string()))));
        assert!(env.contains(&("CLAUDE_CONFIG_DIR".to_string(), None)));
    }

    #[test]
    fn pty_child_does_not_inherit_a_removed_variable() {
        use std::io::Read;
        let _permit = crate::test_subprocess::acquire();
        assert!(std::env::var_os("HOME").is_some(), "test needs an inherited HOME");
        let mut cmd = std::process::Command::new("/bin/sh");
        cmd.args(["-c", "echo \"[${HOME-unset}]\""]).env_remove("HOME");
        let (argv, dir, env) = command_parts(&cmd);
        let mut bridge = crate::pty_bridge::PtyBridge::spawn(&argv, dir.as_deref(), env).unwrap();
        let mut output = String::new();
        bridge.take_reader().unwrap().read_to_string(&mut output).unwrap();
        let _ = bridge.wait().unwrap();
        assert!(output.contains("[unset]"), "child saw HOME: {output}");
    }
}
