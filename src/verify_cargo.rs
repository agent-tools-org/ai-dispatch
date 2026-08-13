// Cargo-backed verification using the shared build runner and target fallback.
// Exports: store-aware Cargo verify entrypoint and target-path classifier.
// Deps: cmd::build runner, VerifyResult, Store.

use anyhow::{Context, Result};
use std::path::Path;

use crate::store::Store;
use crate::verify::{VerifyResult, VERIFY_TIMEOUT};

pub(crate) fn run_verify_with_store(
    store: &Store,
    worktree_path: &Path,
    command: Option<&str>,
    cargo_target_dir: Option<&str>,
    container_name: Option<&str>,
) -> Result<VerifyResult> {
    let Some(request) = crate::cmd::build::BuildRequest::for_verify(worktree_path, command)
        .filter(|_| container_name.is_none())
    else {
        return crate::verify::run_verify(worktree_path, command, cargo_target_dir, container_name);
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
                let _lock = crate::verify::VERIFY_LOCK.lock().unwrap_or_else(|e| e.into_inner());
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
    let mut output = result.report.stderr_lines.join("\n");
    if !result.plain_stdout.is_empty() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(&result.plain_stdout.join("\n"));
    }
    if let Some(note) = result.report.note.as_ref() {
        if !output.is_empty() {
            output.push('\n');
        }
        output.push_str(note);
    }
    if result.timed_out {
        output.push_str(&format!("\nVerification timed out after {} seconds", VERIFY_TIMEOUT.as_secs()));
    }
    Ok(VerifyResult {
        success: result.cargo_success,
        timed_out: result.timed_out,
        output,
        command: result.command,
        infrastructure_failure: result.infrastructure_failure,
    })
}

pub(crate) fn target_dir_permission_failure(result: &VerifyResult, target_dir: Option<&str>) -> bool {
    target_dir.is_some_and(|target| {
        let lines = result.output.lines().map(str::to_string).collect::<Vec<_>>();
        crate::cmd::build::build_fallback::target_dir_permission_blocked(&lines, target)
    })
}
