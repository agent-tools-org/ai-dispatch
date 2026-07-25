// Cargo verification command orchestration for compact agent digests.
// Exports: run().
// Deps: build_diag, build_process, CLI build args, agent cargo-target helpers.

use anyhow::{bail, Result};
use std::sync::Arc;

use crate::cli::command_args_b::{BuildArgs, BuildCommandArg};
use crate::store::Store;

#[path = "build_diag.rs"]
mod build_diag;
#[path = "build_process.rs"]
mod build_process;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum BuildCommand {
    Check,
    Test,
    Clippy,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BuildRequest {
    command: BuildCommand,
    package: Option<String>,
    test_filter: Option<String>,
    include_warnings: bool,
    extra_args: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct CargoTargetChoice {
    value: Option<String>,
    inherited: bool,
}

pub async fn run(store: Arc<Store>, args: BuildArgs) -> Result<i32> {
    if !crate::agent::is_rust_project(None) {
        bail!("This is not a Rust project (no Cargo.toml found).");
    }
    let request = BuildRequest::from_args(args)?;
    let target = resolve_cargo_target_choice(&store);
    let progress = build_process::ProgressConfig::from_env();
    build_process::run_cargo_process(store, request, target, progress).await
}

impl BuildRequest {
    fn from_args(args: BuildArgs) -> Result<Self> {
        let (command, mut extra_args) = default_command_and_args(args.command);
        if args.test_filter.is_some() && command != BuildCommand::Test {
            bail!("--test is only valid with `aid build test`");
        }
        extra_args.extend(args.extra_args);
        Ok(Self {
            command,
            package: args.package,
            test_filter: args.test_filter,
            include_warnings: args.warnings,
            extra_args,
        })
    }

    pub(crate) fn cargo_args(&self) -> Vec<String> {
        let mut args = vec![self.command.as_str().to_string(), "--message-format=json".to_string()];
        if let Some(package) = self.package.as_ref() {
            args.push("-p".to_string());
            args.push(package.clone());
        }
        args.extend(self.extra_args.clone());
        if let Some(filter) = self.test_filter.as_ref() {
            args.push(filter.clone());
        }
        args
    }

    pub(crate) fn display_command(&self, target: &CargoTargetChoice) -> String {
        let mut parts = Vec::new();
        if !target.inherited {
            if let Some(value) = target.value.as_ref() {
                parts.push(crate::agent::cargo_target_env_arg(value));
            }
        }
        parts.push("cargo".to_string());
        parts.extend(self.display_args());
        parts.join(" ")
    }

    fn display_args(&self) -> Vec<String> {
        let mut args = vec![self.command.as_str().to_string()];
        if let Some(package) = self.package.as_ref() {
            args.push("-p".to_string());
            args.push(package.clone());
        }
        args.extend(self.extra_args.clone());
        if let Some(filter) = self.test_filter.as_ref() {
            args.push("--test".to_string());
            args.push(filter.clone());
        }
        args
    }
}

impl BuildCommand {
    fn as_str(self) -> &'static str {
        match self {
            Self::Check => "check",
            Self::Test => "test",
            Self::Clippy => "clippy",
        }
    }
}

fn default_command_and_args(command: Option<BuildCommandArg>) -> (BuildCommand, Vec<String>) {
    if let Some(command) = command {
        return (BuildCommand::from(command), Vec::new());
    }
    let Some(project) = crate::project::detect_project() else {
        return (BuildCommand::Check, Vec::new());
    };
    parse_verify_command(project.verify.as_deref())
}

fn parse_verify_command(verify: Option<&str>) -> (BuildCommand, Vec<String>) {
    let Some(verify) = verify.map(str::trim).filter(|value| !value.is_empty()) else {
        return (BuildCommand::Check, Vec::new());
    };
    let parts = verify.split_whitespace().collect::<Vec<_>>();
    let cargo_args = parts.strip_prefix(&["cargo"]).unwrap_or(&parts);
    match cargo_args.first().copied() {
        Some("check") => (BuildCommand::Check, cargo_args[1..].iter().map(|s| s.to_string()).collect()),
        Some("test") => (BuildCommand::Test, cargo_args[1..].iter().map(|s| s.to_string()).collect()),
        Some("clippy") => (BuildCommand::Clippy, cargo_args[1..].iter().map(|s| s.to_string()).collect()),
        _ => (BuildCommand::Check, Vec::new()),
    }
}

impl From<BuildCommandArg> for BuildCommand {
    fn from(value: BuildCommandArg) -> Self {
        match value {
            BuildCommandArg::Check => Self::Check,
            BuildCommandArg::Test => Self::Test,
            BuildCommandArg::Clippy => Self::Clippy,
        }
    }
}

fn resolve_cargo_target_choice(store: &Store) -> CargoTargetChoice {
    if let Ok(value) = std::env::var("CARGO_TARGET_DIR") {
        return CargoTargetChoice { value: Some(value), inherited: true };
    }
    let branch = task_branch(store).or_else(current_branch);
    CargoTargetChoice {
        value: crate::agent::target_dir_for_worktree(branch.as_deref()),
        inherited: false,
    }
}

fn task_branch(store: &Store) -> Option<String> {
    let task_id = std::env::var("AID_TASK_ID").ok()?;
    store.get_task(&task_id).ok().flatten()?.worktree_branch
}

fn current_branch() -> Option<String> {
    let cwd = std::env::current_dir().ok()?;
    let output = std::process::Command::new("git")
        .args(["-C", &cwd.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let branch = String::from_utf8(output.stdout).ok()?.trim().to_string();
    (branch != "HEAD").then_some(branch)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(command: Option<BuildCommandArg>) -> BuildArgs {
        BuildArgs {
            command,
            package: None,
            test_filter: None,
            warnings: false,
            extra_args: Vec::new(),
        }
    }

    #[test]
    fn request_builds_check_cargo_args() {
        let mut cli_args = args(Some(BuildCommandArg::Check));
        cli_args.package = Some("ai-dispatch".to_string());
        cli_args.extra_args = vec!["--all-targets".to_string()];
        let request = BuildRequest::from_args(cli_args).expect("valid check request");
        assert_eq!(
            request.cargo_args(),
            ["check", "--message-format=json", "-p", "ai-dispatch", "--all-targets"]
        );
    }

    #[test]
    fn request_rejects_test_filter_for_check() {
        let mut cli_args = args(Some(BuildCommandArg::Check));
        cli_args.test_filter = Some("smoke".to_string());
        assert!(BuildRequest::from_args(cli_args).is_err());
    }

    #[test]
    fn request_places_test_filter_as_cargo_test_filter() {
        let mut cli_args = args(Some(BuildCommandArg::Test));
        cli_args.test_filter = Some("retry_flow".to_string());
        let request = BuildRequest::from_args(cli_args).expect("valid test request");
        assert_eq!(request.cargo_args(), ["test", "--message-format=json", "retry_flow"]);
    }

    #[test]
    fn verify_config_selects_supported_cargo_command() {
        let (command, args) = parse_verify_command(Some("cargo clippy --all-targets"));
        assert_eq!(command, BuildCommand::Clippy);
        assert_eq!(args, ["--all-targets"]);
    }

    #[test]
    fn display_command_marks_only_non_inherited_target_dir() {
        let request = BuildRequest::from_args(args(Some(BuildCommandArg::Check))).expect("valid check request");
        let inherited = CargoTargetChoice { value: Some("/tmp/warm".to_string()), inherited: true };
        assert_eq!(request.display_command(&inherited), "cargo check");

        let explicit = CargoTargetChoice { value: Some("/tmp/warm".to_string()), inherited: false };
        assert_eq!(
            request.display_command(&explicit),
            "CARGO_TARGET_DIR=/tmp/warm cargo check"
        );
    }
}
