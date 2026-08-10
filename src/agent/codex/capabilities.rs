// Codex CLI contract validation for host-side dispatch preflight.
// Exports: validate_installed_codex; rejects obsolete approval flag surfaces.
// Deps: anyhow and the installed `codex exec --help` output.

use anyhow::{Context, Result, ensure};
use std::process::Command;

use crate::agent::{CliCommandOutput, CliCommandRunner};

const APPROVE_FOR_ME_VERSION: (u32, u32, u32) = (0, 147, 0);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ApprovalFlag {
    ApproveForMe,
    FullAuto,
}

impl ApprovalFlag {
    pub(super) const fn as_str(self) -> &'static str {
        match self {
            Self::ApproveForMe => "--approve-for-me",
            Self::FullAuto => "--full-auto",
        }
    }
}

pub(super) fn approval_flag_for_version(version: (u32, u32, u32)) -> ApprovalFlag {
    if version >= APPROVE_FOR_ME_VERSION {
        ApprovalFlag::ApproveForMe
    } else {
        ApprovalFlag::FullAuto
    }
}

pub(super) fn validate_installed_codex(version: Option<(u32, u32, u32)>) -> Result<()> {
    validate_installed_codex_with(version, &run_codex_command)
}

pub(super) fn validate_installed_codex_with(
    version: Option<(u32, u32, u32)>,
    run: &CliCommandRunner<'_>,
) -> Result<()> {
    let Some(version) = version else {
        eprintln!("warning: could not read Codex CLI version; skipping flag validation");
        return Ok(());
    };
    let output = match run("codex", &["exec", "--help"]) {
        Ok(output) if output.success => output,
        Ok(_) => {
            eprintln!("warning: `codex exec --help` failed; skipping flag validation");
            return Ok(());
        }
        Err(error) => {
            eprintln!("warning: could not inspect Codex CLI flags ({error:#}); skipping validation");
            return Ok(());
        }
    };
    validate_exec_help(version, &format!("{}{}", output.stdout, output.stderr))
}

fn run_codex_command(program: &str, args: &[&str]) -> Result<CliCommandOutput> {
    let output = Command::new(program)
        .args(args)
        .output()
        .context("failed to inspect Codex CLI capabilities")?;
    Ok(CliCommandOutput {
        success: output.status.success(),
        stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
        stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
    })
}

fn validate_exec_help(version: (u32, u32, u32), help: &str) -> Result<()> {
    let required = approval_flag_for_version(version).as_str();
    ensure!(
        help_defines_flag(help, required),
        "Codex CLI {}.{}.{} does not define required flag {required}",
        version.0,
        version.1,
        version.2,
    );
    Ok(())
}

fn help_defines_flag(help: &str, flag: &str) -> bool {
    help.lines().any(|line| {
        line.trim_start().strip_prefix(flag).is_some_and(|rest| {
            rest.is_empty() || rest.starts_with([' ', '\t', '=', ','])
        })
    })
}

#[cfg(test)]
mod tests {
    use super::{ApprovalFlag, approval_flag_for_version, validate_exec_help, validate_installed_codex_with};
    use crate::agent::CliCommandOutput;
    use std::cell::RefCell;

    #[test]
    fn selects_flags_for_old_and_new_versions() {
        assert_eq!(approval_flag_for_version((0, 146, 0)), ApprovalFlag::FullAuto);
        assert_eq!(approval_flag_for_version((0, 147, 0)), ApprovalFlag::ApproveForMe);
        assert_eq!(approval_flag_for_version((1, 0, 0)), ApprovalFlag::ApproveForMe);
    }

    #[test]
    fn validates_the_flag_selected_for_each_version() {
        assert!(validate_exec_help((0, 146, 0), "      --full-auto\n").is_ok());
        assert!(validate_exec_help((0, 147, 0), "      --approve-for-me\n").is_ok());
        let error = validate_exec_help((0, 147, 0), "      --full-auto\n").unwrap_err();
        assert!(error.to_string().contains("--approve-for-me"));
    }

    #[test]
    fn validates_codex_help_through_the_injected_runner() {
        let invocation = RefCell::new(None);
        let runner = |program: &str, args: &[&str]| {
            *invocation.borrow_mut() = Some((
                program.to_string(),
                args.iter().map(|arg| (*arg).to_string()).collect::<Vec<_>>(),
            ));
            Ok(CliCommandOutput {
                success: true,
                stdout: "--approve-for-me\n".to_string(),
                stderr: String::new(),
            })
        };

        validate_installed_codex_with(Some((0, 147, 0)), &runner).unwrap();

        assert_eq!(
            *invocation.borrow(),
            Some((
                "codex".to_string(),
                vec!["exec".to_string(), "--help".to_string()]
            ))
        );
    }

    #[test]
    fn unknown_version_skips_flag_validation() {
        let runner = |_program: &str, _args: &[&str]| -> anyhow::Result<CliCommandOutput> {
            panic!("help must not be required when the version is unknown")
        };

        validate_installed_codex_with(None, &runner).unwrap();
    }

    #[test]
    fn failed_help_probe_does_not_block_a_known_version() {
        let runner = |_program: &str, _args: &[&str]| {
            Ok(CliCommandOutput {
                success: false,
                stdout: String::new(),
                stderr: "unsupported".to_string(),
            })
        };

        validate_installed_codex_with(Some((0, 147, 0)), &runner).unwrap();
    }

    #[test]
    fn known_version_still_rejects_a_missing_required_flag() {
        let runner = |_program: &str, _args: &[&str]| {
            Ok(CliCommandOutput {
                success: true,
                stdout: "--full-auto\n".to_string(),
                stderr: String::new(),
            })
        };

        let error = validate_installed_codex_with(Some((0, 147, 0)), &runner).unwrap_err();
        assert!(error.to_string().contains("--approve-for-me"));
    }
}
