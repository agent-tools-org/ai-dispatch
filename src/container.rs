// Reusable dev container helpers for task execution and verification.
// Exports lifecycle helpers, exec wrappers, and small command builders.
// Deps: anyhow, std::process::Command, std::path::Path.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

const CONTAINER_BIN: &str = "container";
const CONTAINER_WORKER: &str = "sleep";
const CONTAINER_WORKER_ARG: &str = "infinity";

pub fn container_name(name: &str) -> String {
    let mut value = String::from("aid-dev-");
    for ch in name.chars().flat_map(char::to_lowercase) {
        if ch.is_ascii_alphanumeric() {
            value.push(ch);
        } else if matches!(ch, '-' | '_' | '.') && !value.ends_with('-') {
            value.push('-');
        }
    }
    value.trim_end_matches('-').to_string()
}

pub fn start_or_reuse(image: &str, project_dir: &Path, project_id: &str) -> Result<String> {
    let project_dir = canonical_dir(project_dir)?;
    let name = container_name(project_id);
    if is_running(&name) {
        return Ok(name);
    }

    stop_container(&name);
    let mut cmd = Command::new(CONTAINER_BIN);
    cmd.arg("run")
        .arg("-d")
        .arg("--rm")
        .arg("--init")
        .arg("--name")
        .arg(&name)
        .arg("-v")
        .arg(format!(
            "{}:{}",
            project_dir.display(),
            project_dir.display()
        ))
        .arg("-w")
        .arg(&project_dir);
    for arg in mount_home_dirs_from(home_dir()) {
        cmd.arg(arg);
    }
    let status = cmd
        .arg(image)
        .arg(CONTAINER_WORKER)
        .arg(CONTAINER_WORKER_ARG)
        .status()
        .with_context(|| format!("Failed to start container '{name}'"))?;
    if !status.success() {
        anyhow::bail!("Failed to start container '{name}'");
    }
    Ok(name)
}

pub fn exec_in_container(cmd: &Command, container_name: &str) -> Command {
    let cwd = cmd
        .get_current_dir()
        .map(Path::to_path_buf)
        .or_else(|| std::env::current_dir().ok());
    let mut wrapped = Command::new(CONTAINER_BIN);
    wrapped.arg("exec");
    if let Some(dir) = cwd.as_deref() {
        wrapped.arg("-w").arg(dir);
        wrapped.current_dir(dir);
    }
    for (key, value) in cmd.get_envs() {
        if let Some(value) = value {
            wrapped.arg("-e").arg(format!(
                "{}={}",
                key.to_string_lossy(),
                value.to_string_lossy()
            ));
        }
    }
    wrapped.arg(container_name);
    wrapped.arg(cmd.get_program());
    wrapped.args(cmd.get_args());
    wrapped
}

pub fn verify_in_container(
    container_name: &str,
    worktree_path: &Path,
    verify_cmd: &str,
    cargo_target_dir: Option<&str>,
) -> Command {
    let mut cmd = Command::new(CONTAINER_BIN);
    cmd.arg("exec");
    if let Some(cargo_target_dir) = cargo_target_dir {
        cmd.arg("-e")
            .arg(format!("CARGO_TARGET_DIR={cargo_target_dir}"));
    }
    cmd.arg("-w")
        .arg(worktree_path)
        .arg(container_name)
        .args(["sh", "-c", verify_cmd]);
    cmd
}

pub fn stop_container(name: &str) {
    let _ = Command::new(CONTAINER_BIN)
        .args(["kill", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    let _ = Command::new(CONTAINER_BIN)
        .args(["rm", name])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

pub fn build_image(tag: &str, file: Option<&str>) -> Result<()> {
    let mut cmd = Command::new(CONTAINER_BIN);
    cmd.arg("build").arg("-t").arg(tag);
    if let Some(file) = file {
        cmd.arg("-f").arg(file);
    }
    let status = cmd.arg(".").status()?;
    if !status.success() {
        anyhow::bail!("container build failed for tag '{tag}'");
    }
    Ok(())
}

pub fn list_containers() -> Result<()> {
    let status = Command::new(CONTAINER_BIN).arg("ps").status()?;
    if !status.success() {
        anyhow::bail!("container ps failed");
    }
    Ok(())
}

fn canonical_dir(project_dir: &Path) -> Result<PathBuf> {
    project_dir
        .canonicalize()
        .with_context(|| format!("Failed to resolve {}", project_dir.display()))
}

fn is_running(name: &str) -> bool {
    Command::new(CONTAINER_BIN)
        .args(["exec", name, "true"])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .map(|status| status.success())
        .unwrap_or(false)
}

fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME").map(PathBuf::from)
}

fn mount_home_dirs_from(home: Option<PathBuf>) -> Vec<String> {
    let mut args = Vec::new();
    let Some(home) = home else { return args };
    for subdir in [".codex", ".gemini", ".kilo", ".codebuff", ".opencode"] {
        let host_path = home.join(subdir);
        if host_path.exists() {
            let container_path = Path::new("/root").join(subdir);
            args.push("-v".to_string());
            args.push(format!(
                "{}:{}",
                host_path.display(),
                container_path.display()
            ));
        }
    }
    args
}

#[cfg(test)]
mod tests {
    use super::{container_name, exec_in_container, mount_home_dirs_from};
    use std::fs;
    use std::process::Command;
    use tempfile::TempDir;

    fn args(cmd: &Command) -> Vec<String> {
        cmd.get_args()
            .map(|arg| arg.to_string_lossy().into_owned())
            .collect()
    }

    #[test]
    fn container_name_formats_input() {
        assert_eq!(container_name("Project.Name_01"), "aid-dev-project-name-01");
    }

    #[test]
    fn mount_home_dirs_returns_expected_args() {
        let home = TempDir::new().unwrap();
        fs::create_dir(home.path().join(".codex")).unwrap();
        fs::create_dir(home.path().join(".gemini")).unwrap();

        let args = mount_home_dirs_from(Some(home.path().to_path_buf()));

        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-v" && pair[1].contains(".codex:/root/.codex")));
        assert!(args
            .windows(2)
            .any(|pair| pair[0] == "-v" && pair[1].contains(".gemini:/root/.gemini")));
    }

    #[test]
    fn exec_in_container_builds_correct_command_structure() {
        let mut cmd = Command::new("codex");
        cmd.current_dir("/tmp/project");
        cmd.env("OPENAI_API_KEY", "test-key");
        cmd.args(["exec", "ship it"]);

        let wrapped = exec_in_container(&cmd, "aid-dev-demo");
        let wrapped_args = args(&wrapped);

        assert_eq!(wrapped.get_program().to_string_lossy(), "container");
        assert_eq!(wrapped_args[0], "exec");
        assert!(wrapped_args.windows(2).any(|pair| pair == ["-w", "/tmp/project"]));
        assert!(wrapped_args
            .windows(2)
            .any(|pair| pair == ["-e", "OPENAI_API_KEY=test-key"]));
        assert_eq!(wrapped_args[wrapped_args.len() - 4], "aid-dev-demo");
        assert_eq!(wrapped_args[wrapped_args.len() - 3], "codex");
        assert_eq!(wrapped_args[wrapped_args.len() - 2], "exec");
        assert_eq!(wrapped_args[wrapped_args.len() - 1], "ship it");
    }
}
