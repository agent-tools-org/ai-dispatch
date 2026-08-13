// Cargo verification command orchestration for compact agent digests.
// Exports: run().
// Deps: build_diag, build_process, CLI build args, agent cargo-target helpers.

use anyhow::{bail, Result};
use std::path::Path;
use std::sync::Arc;

use crate::cli::command_args_b::{BuildArgs, BuildCommandArg};
use crate::store::Store;

#[path = "build_diag.rs"]
pub(crate) mod build_diag;
#[path = "build_fallback.rs"]
pub(crate) mod build_fallback;
#[path = "build_stream.rs"]
mod build_stream;
#[path = "build_process.rs"]
pub(crate) mod build_process;
#[path = "build_progress.rs"]
mod build_progress;

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

/// Shared target-dir resolution for `aid build` and `aid test`.
pub(crate) fn resolve_target(store: &Store) -> CargoTargetChoice {
    resolve_cargo_target_choice(store)
}

pub(crate) fn target_for_verify(cargo_target_dir: Option<&str>) -> CargoTargetChoice {
    let value = cargo_target_dir.map(str::to_string).or_else(|| std::env::var("CARGO_TARGET_DIR").ok());
    CargoTargetChoice { value, inherited: cargo_target_dir.is_none() }
}
impl BuildRequest {
    fn from_args(args: BuildArgs) -> Result<Self> {
        let (command, mut extra_args) = default_command_and_args(args.command);
        extra_args.extend(args.extra_args);
        Ok(Self {
            command,
            package: args.package,
            test_filter: None,
            include_warnings: args.warnings,
            extra_args,
        })
    }

    /// Build a cargo-test request used by `aid test` (not the CLI `build` surface).
    pub(crate) fn for_test(
        package: Option<String>,
        extra_args: Vec<String>,
        test_filter: Option<String>,
        include_warnings: bool,
    ) -> Self {
        Self {
            command: BuildCommand::Test,
            package,
            test_filter,
            include_warnings,
            extra_args,
        }
    }

    pub(crate) fn for_verify(worktree_path: &Path, command: Option<&str>) -> Option<Self> {
        let command = match command {
            Some(command) => command.trim().to_string(),
            None if worktree_path.join("Cargo.toml").exists() => "cargo check".to_string(),
            None => return None,
        };
        let parts = command.split_whitespace().collect::<Vec<_>>();
        let cargo_args = parts.strip_prefix(&["cargo"])?;
        let (command, extra_args) = match cargo_args.first().copied()? {
            "check" => (BuildCommand::Check, &cargo_args[1..]),
            "test" => (BuildCommand::Test, &cargo_args[1..]),
            "clippy" => (BuildCommand::Clippy, &cargo_args[1..]),
            _ => return None,
        };
        Some(Self {
            command,
            package: None,
            test_filter: None,
            include_warnings: false,
            extra_args: extra_args.iter().map(|arg| (*arg).to_string()).collect(),
        })
    }

    pub(crate) fn include_warnings(&self) -> bool {
        self.include_warnings
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
        // Project verify may be `cargo test …`; compile checks stay on `aid build`.
        // Trusted test runs go through `aid test` (libtest guarantees).
        Some("test") => (BuildCommand::Check, cargo_args[1..].iter().map(|s| s.to_string()).collect()),
        Some("clippy") => (BuildCommand::Clippy, cargo_args[1..].iter().map(|s| s.to_string()).collect()),
        _ => (BuildCommand::Check, Vec::new()),
    }
}

impl From<BuildCommandArg> for BuildCommand {
    fn from(value: BuildCommandArg) -> Self {
        match value {
            BuildCommandArg::Check => Self::Check,
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
    fn for_test_places_filter_as_cargo_test_filter() {
        let request = BuildRequest::for_test(
            None,
            vec!["--bin".to_string(), "aid".to_string()],
            Some("retry_flow".to_string()),
            false,
        );
        assert_eq!(
            request.cargo_args(),
            ["test", "--message-format=json", "--bin", "aid", "retry_flow"]
        );
    }

    #[test]
    fn verify_config_selects_supported_cargo_command() {
        let (command, args) = parse_verify_command(Some("cargo clippy --all-targets"));
        assert_eq!(command, BuildCommand::Clippy);
        assert_eq!(args, ["--all-targets"]);
    }

    #[test]
    fn verify_cargo_test_maps_to_check_not_test() {
        let (command, args) = parse_verify_command(Some("cargo test --bin aid"));
        assert_eq!(command, BuildCommand::Check);
        assert_eq!(args, ["--bin", "aid"]);
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

#[cfg(test)] #[path = "build_verify_tests.rs"] mod verify_tests;
