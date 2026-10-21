// Cargo-backed verification using the shared build runner and target fallback.
// Exports: store-aware Cargo verify entrypoint and target-path classifier.
// Deps: cmd::build runner, VerifyResult, Store.

use anyhow::{Context, Result};
use std::path::Path;

use crate::store::Store;
use crate::task::verify::{VerifyResult, VERIFY_TIMEOUT};

pub(crate) fn run_verify_with_store(
    store: &Store,
    worktree_path: &Path,
    command: Option<&str>,
    cargo_target_dir: Option<&str>,
    container_name: Option<&str>,
) -> Result<VerifyResult> {
    let Some(request) = crate::cmd::build::BuildRequest::for_verify(worktree_path, command)
        .filter(|_| container_name.is_none() && command.is_none_or(plain_command))
    else {
        return crate::task::verify::run_verify(worktree_path, command, cargo_target_dir, container_name);
    };
    let target = crate::cmd::build::target_for_verify(cargo_target_dir);
    let worktree_path = worktree_path.to_path_buf();
    let result = std::thread::scope(|scope| {
        scope
            .spawn(move || -> Result<_> {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .context("Failed to create Cargo verification runtime")?;
                let _lock = crate::task::verify::VERIFY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
                runtime.block_on(crate::cmd::build::build_process::run_cargo_outcome(
                    store,
                    request,
                    target,
                    crate::cmd::build::build_process::ProgressConfig::from_env(),
                    Some(&worktree_path),
                    Some(VERIFY_TIMEOUT),
                    &[],
                ))
            })
            .join()
            .map_err(|_| anyhow::anyhow!("Cargo verification thread panicked"))?
    })?;
    let mut output = cargo_output(
        &result.report.stderr_lines, &result.plain_stdout, result.report.note.as_deref(),
    );
    if result.timed_out {
        output.push_str(&format!("\nVerification timed out after {} seconds", VERIFY_TIMEOUT.as_secs()));
    }
    Ok(VerifyResult {
        success: result.cargo_success,
        timed_out: result.timed_out,
        output,
        command: result.command,
        infrastructure_failure: result.infrastructure_failure,
        rbox_result: None,
        exit_code: Some(result.exit_code),
    })
}

fn cargo_output(stderr: &[String], stdout: &[String], note: Option<&str>) -> String {
    let mut output = stderr.join("\n");
    if !stdout.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&stdout.join("\n"));
    }
    if let Some(note) = note {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(note);
    }
    output
}

fn plain_command(command: &str) -> bool {
    command.bytes().all(|byte| {
        byte.is_ascii_alphanumeric() || b" \t/._-:=,+%@".contains(&byte)
    })
}

#[cfg(test)]
mod tests {
    use super::plain_command;

    #[test]
    fn cargo_fast_path_accepts_plain_arguments() {
        for command in ["cargo check", "cargo test --bin aid", " cargo clippy\t-p example "] {
            assert!(plain_command(command), "{command}");
        }
    }

    #[test]
    fn cargo_fast_path_rejects_shell_syntax() {
        for syntax in [
            "&& false", "|| true", "; false", "| cat", "&", "> out", "< in",
            "'a b'", "\"a b\"", "$(true)", "`true`", "$FILTER", "*", "?", "[ab]",
            "{a,b}", "~/dir", "a\\ b", "\nfalse", "# comment", "(true)", "! true",
        ] {
            let command = format!("cargo test {syntax}");
            assert!(!plain_command(&command), "{command}");
        }
    }
}

pub(crate) fn target_dir_permission_failure(result: &VerifyResult, target_dir: Option<&str>) -> bool {
    target_dir.is_some_and(|target| {
        let lines = result.output.lines().map(str::to_string).collect::<Vec<_>>();
        crate::cmd::build::build_fallback::target_dir_permission_blocked(&lines, target)
    })
}
