// Trusted `aid test` command: cargo test with libtest guarantees.
// Exports: run().
// Deps: build process/diag, test_parse, CLI TestArgs, Store.

use anyhow::{bail, Context, Result};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use super::build::build_diag::render_digest;
use super::build::build_process::{self, CargoRunOutcome};
use super::build::{resolve_target, BuildRequest};
use super::test_parse::{evaluate_test_run, parse_libtest_lines};
use crate::cli::command_args_b::TestArgs;
use crate::store::Store;
use crate::types::{EventKind, TaskEvent, TaskId};

pub async fn run(store: Arc<Store>, args: TestArgs) -> Result<i32> {
    if !crate::agent::is_rust_project(None) {
        bail!("This is not a Rust project (no Cargo.toml found).");
    }
    let filter = args.filter.clone();
    let request = build_request(&args);
    let target = resolve_target(&store);
    let progress = build_process::ProgressConfig::from_env();
    let isolated = if args.isolated {
        Some(IsolatedHome::create()?)
    } else {
        None
    };
    let child_env = isolated
        .as_ref()
        .map(|dir| vec![("AID_HOME".to_string(), dir.path().to_string_lossy().into_owned())])
        .unwrap_or_default();
    let outcome =
        build_process::run_cargo_outcome(store.clone(), request, target, progress, &child_env)
            .await?;
    let verdict = verdict_from_outcome(&outcome, filter.as_deref(), args.warnings);
    println!("{}", verdict.digest);
    emit_finished(&store, &verdict.digest);
    // Keep the temp AID_HOME alive for the whole cargo child lifetime.
    drop(isolated);
    Ok(verdict.exit_code)
}

/// Temporary AID_HOME for `--isolated` runs; removed on drop.
struct IsolatedHome {
    path: PathBuf,
}

impl IsolatedHome {
    fn create() -> Result<Self> {
        let nanos = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        let path = std::env::temp_dir().join(format!("aid-test-home-{}-{nanos}", std::process::id()));
        std::fs::create_dir_all(&path)
            .with_context(|| format!("Failed to create isolated AID_HOME at {}", path.display()))?;
        Ok(Self { path })
    }

    fn path(&self) -> &std::path::Path {
        &self.path
    }
}

impl Drop for IsolatedHome {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.path);
    }
}

fn build_request(args: &TestArgs) -> BuildRequest {
    // cargo test [CARGO_OPTS] [FILTER] [-- HARNESS_OPTS]
    let mut extra = Vec::new();
    if let Some(bin) = args.bin.as_ref() {
        extra.push("--bin".to_string());
        extra.push(bin.clone());
    }
    if args.lib {
        extra.push("--lib".to_string());
    }
    if let Some(test_target) = args.test_target.as_ref() {
        extra.push("--test".to_string());
        extra.push(test_target.clone());
    }
    if let Some(filter) = args.filter.as_ref() {
        extra.push(filter.clone());
    }
    // clap `last = true` args are harness passthrough after `--`.
    if !args.extra_args.is_empty() {
        extra.push("--".to_string());
        extra.extend(args.extra_args.clone());
    }
    // Filter is embedded in extra for correct cargo ordering; evaluate_test_run
    // still receives the original filter string for guarantee messages.
    BuildRequest::for_test(args.package.clone(), extra, None, args.warnings)
}

fn verdict_from_outcome(
    outcome: &CargoRunOutcome,
    filter: Option<&str>,
    include_warnings: bool,
) -> super::test_parse::TestVerdict {
    let summary = parse_libtest_lines(&outcome.plain_stdout);
    let compiler_lines = render_digest(&outcome.report, include_warnings)
        .lines()
        .skip(1) // drop build outcome headline; test digest has its own
        .filter(|line| !line.is_empty())
        .map(str::to_string)
        .collect::<Vec<_>>();
    evaluate_test_run(
        &summary,
        filter,
        outcome.compiled_units,
        outcome.cargo_success,
        &outcome.command,
        outcome.elapsed,
        &compiler_lines,
    )
}

fn emit_finished(store: &Store, detail: &str) {
    let Some(task_id) = std::env::var("AID_TASK_ID").ok() else {
        return;
    };
    let headline = detail.lines().next().unwrap_or(detail);
    let _ = store.insert_event(&TaskEvent {
        task_id: TaskId(task_id),
        timestamp: chrono::Local::now(),
        event_kind: EventKind::Build,
        detail: headline.to_string(),
        metadata: None,
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn build_request_orders_bin_filter_and_harness_args() {
        let args = TestArgs {
            package: None,
            bin: Some("aid".to_string()),
            lib: false,
            test_target: None,
            filter: Some("paths::".to_string()),
            isolated: false,
            warnings: false,
            extra_args: vec!["--exact".to_string()],
        };
        let request = build_request(&args);
        assert_eq!(
            request.cargo_args(),
            [
                "test",
                "--message-format=json",
                "--bin",
                "aid",
                "paths::",
                "--",
                "--exact",
            ]
        );
    }

    #[test]
    fn build_request_supports_lib_and_package() {
        let args = TestArgs {
            package: Some("ai-dispatch".to_string()),
            bin: None,
            lib: true,
            test_target: None,
            filter: None,
            isolated: false,
            warnings: true,
            extra_args: Vec::new(),
        };
        let request = build_request(&args);
        assert_eq!(
            request.cargo_args(),
            ["test", "--message-format=json", "-p", "ai-dispatch", "--lib"]
        );
        assert!(request.include_warnings());
    }
}
