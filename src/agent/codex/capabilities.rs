// Codex CLI options, native read-only permissions and dispatch preflight.
// Exports option configuration and installed approval-flag validation.
// Deps: RunOpts, TOML configuration and `codex exec --help` output.

use anyhow::{Result, ensure};
use crate::agent::env_identity::help_defines_flag;

use crate::agent::CliCommandRunner;

const APPROVE_FOR_ME_VERSION: (u32, u32, u32) = (0, 147, 0);

fn configure_read_only(
    cmd: &mut std::process::Command,
    roots: &[std::path::PathBuf],
) -> Result<()> {
    let mut filesystem = toml::Table::new();
    for path in roots {
        let path = path.to_str().ok_or_else(|| anyhow::anyhow!("Non-UTF-8 sandbox root"))?;
        filesystem.insert(path.to_string(), toml::Value::String("write".into()));
    }
    let profile = toml::Value::Table(toml::Table::from_iter([
        ("extends".into(), toml::Value::String(":read-only".into())),
        ("filesystem".into(), toml::Value::Table(filesystem)),
    ]));
    let permissions = toml::Value::Table(toml::Table::from_iter([("aid-read-only".into(), profile)]));
    cmd.args(["-c", "sandbox_mode=\"read-only\""]);
    cmd.args(["-c", &format!("permissions={permissions}")]);
    cmd.args(["-c", "default_permissions=\"aid-read-only\"", "-c", "approval_policy=\"never\""]);
    Ok(())
}

pub(super) fn configure_options(
    cmd: &mut std::process::Command,
    opts: &crate::agent::RunOpts,
    resuming: bool,
    roots: &[std::path::PathBuf],
) -> Result<()> {
    if let Some(ref model) = opts.model {
        if super::has_native_model_flag() {
            cmd.args(["-m", model]);
        } else {
            cmd.args(["-c", &format!("model=\"{model}\"")]);
        }
    }
    if let Some(ref output) = opts.output {
        cmd.args(["-o", output]);
    }
    if let Some(ref dir) = opts.dir {
        if !std::path::Path::new(dir).exists() {
            anyhow::bail!("codex working directory does not exist: {}", dir);
        }
        if !resuming {
            cmd.args(["-C", dir]);
        }
        cmd.current_dir(dir);
    }
    if opts.read_only {
        configure_read_only(cmd, roots)
    } else {
        crate::agent::scratch::grant_codex_roots(cmd, roots)
    }
}

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
    fn timed_out_help_probe_is_a_soft_skip_for_a_known_version() {
        let runner = |_program: &str, _args: &[&str]| {
            crate::agent::env_identity::run_bounded("sh", &["-c", "exec sleep 60"], std::time::Duration::from_millis(50))
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
