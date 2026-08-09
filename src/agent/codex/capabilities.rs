// Codex CLI contract validation for host-side dispatch preflight.
// Exports: validate_installed_codex; rejects obsolete approval flag surfaces.
// Deps: anyhow and the installed `codex exec --help` output.

use anyhow::{Context, Result, ensure};
use std::process::Command;

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

pub(super) fn validate_installed_codex(version: (u32, u32, u32)) -> Result<()> {
    let output = Command::new("codex")
        .args(["exec", "--help"])
        .output()
        .context("failed to inspect Codex CLI capabilities")?;
    ensure!(
        output.status.success(),
        "`codex exec --help` failed; reinstall or upgrade Codex CLI"
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    validate_exec_help(version, &format!("{stdout}{stderr}"))
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
    use super::{ApprovalFlag, approval_flag_for_version, validate_exec_help};

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
}
