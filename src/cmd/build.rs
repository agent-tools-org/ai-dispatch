// Cargo build and check orchestration for agent context reduction.
// Exports: run().
// Deps: tokio::process, serde_json, anyhow, std::process, std::collections.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, BufReader};
use tokio::process::Command;

use crate::store::Store;

#[derive(Debug, serde::Deserialize)]
struct CompilerMessageReason {
    message: Message,
}

#[derive(Debug, serde::Deserialize)]
struct Message {
    level: String,
    message: String,
    #[serde(default)]
    spans: Vec<Span>,
}

#[derive(Debug, serde::Deserialize)]
struct Span {
    file_name: String,
    line_start: usize,
    is_primary: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Hash)]
struct DeduppedMessage {
    level: String,
    file_name: String,
    line: usize,
    message: String,
}

pub async fn run(store: Arc<Store>, args: Vec<String>) -> Result<i32> {
    if !crate::agent::is_rust_project(None) {
        anyhow::bail!("This is not a Rust project (no Cargo.toml found).");
    }

    let target_dir = resolve_target_dir(&store);
    let (subcommand, cargo_args) = resolve_cargo_args(&args)?;

    let task_id = std::env::var("AID_TASK_ID").ok();
    let progress_interval_ms = std::env::var("AID_BUILD_PROGRESS_INTERVAL_MS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .unwrap_or(10_000);

    run_cargo_process(
        &subcommand,
        cargo_args,
        target_dir,
        store,
        task_id,
        progress_interval_ms,
    )
    .await
}

fn resolve_target_dir(store: &Store) -> Option<String> {
    let mut cargo_target_dir = std::env::var("CARGO_TARGET_DIR").ok();
    if cargo_target_dir.is_none() {
        let task_branch = if let Ok(task_id_str) = std::env::var("AID_TASK_ID") {
            store.get_task(&task_id_str).ok().flatten().and_then(|t| t.worktree_branch)
        } else {
            None
        };
        
        let branch = task_branch.or_else(|| {
            std::env::current_dir().ok().and_then(|cwd| current_branch(&cwd))
        });
        
        cargo_target_dir = crate::agent::target_dir_for_worktree(branch.as_deref());
    }
    cargo_target_dir
}

fn current_branch(repo_dir: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if branch == "HEAD" { return None; }
    Some(branch)
}

fn resolve_cargo_args(args: &Vec<String>) -> Result<(String, Vec<String>)> {
    let mut cargo_args = if !args.is_empty() {
        args.clone()
    } else {
        let verify_cmd = crate::project::detect_project().and_then(|p| p.verify);
        if let Some(cmd_str) = verify_cmd {
            let trimmed = cmd_str.trim();
            if let Some(rest) = trimmed.strip_prefix("cargo ") {
                rest.split_whitespace().map(|s| s.to_string()).collect()
            } else if trimmed == "cargo" {
                vec!["check".to_string()]
            } else {
                eprintln!("[aid] Warning: project verify command '{}' is not a cargo command. Defaulting to 'cargo check'.", trimmed);
                vec!["check".to_string()]
            }
        } else {
            vec!["check".to_string()]
        }
    };

    if !cargo_args.is_empty() && cargo_args[0] == "cargo" {
        cargo_args.remove(0);
    }
    if cargo_args.is_empty() {
        cargo_args.push("check".to_string());
    }

    let subcommand = cargo_args[0].clone();

    if let Some(pos) = cargo_args.iter().position(|x| x == "--") {
        cargo_args.insert(pos, "--message-format=json".to_string());
    } else {
        if cargo_args.len() > 1 {
            cargo_args.insert(1, "--message-format=json".to_string());
        } else {
            cargo_args.push("--message-format=json".to_string());
        }
    }

    Ok((subcommand, cargo_args))
}

async fn run_cargo_process(
    subcommand: &str,
    cargo_args: Vec<String>,
    target_dir: Option<String>,
    store: Arc<Store>,
    task_id: Option<String>,
    progress_interval_ms: u64,
) -> Result<i32> {
    let mut cmd = Command::new("cargo");
    cmd.args(&cargo_args);
    if let Some(ref target_dir) = target_dir {
        cmd.env("CARGO_TARGET_DIR", target_dir);
    }
    cmd.stdout(std::process::Stdio::piped());
    cmd.stderr(std::process::Stdio::piped());

    let mut child = cmd.spawn().context("Failed to spawn cargo process")?;
    let stdout = child.stdout.take().context("Failed to get cargo stdout")?;
    let stderr = child.stderr.take().context("Failed to get cargo stderr")?;

    if let Some(tid) = task_id.as_ref() {
        let _ = store.insert_event(&crate::types::TaskEvent {
            task_id: crate::types::TaskId(tid.clone()),
            timestamp: chrono::Local::now(),
            event_kind: crate::types::EventKind::Build,
            detail: format!("cargo {} started", subcommand),
            metadata: None,
        });
    }

    let (ordered_messages, stderr_captured, success) = read_streams(
        &mut child,
        stdout,
        stderr,
        &store,
        &task_id,
        progress_interval_ms,
    ).await?;

    print_digest(subcommand, &ordered_messages, &stderr_captured, success);

    if let Some(tid) = task_id.as_ref() {
        let mut error_count = 0;
        let mut warning_count = 0;
        for msg in &ordered_messages {
            if msg.level == "error" {
                error_count += 1;
            } else if msg.level == "warning" {
                warning_count += 1;
            }
        }
        let _ = store.insert_event(&crate::types::TaskEvent {
            task_id: crate::types::TaskId(tid.clone()),
            timestamp: chrono::Local::now(),
            event_kind: crate::types::EventKind::Build,
            detail: format!("cargo {} finished: {} errors, {} warnings", subcommand, error_count, warning_count),
            metadata: None,
        });
    }

    let status = child.wait().await?;
    Ok(status.code().unwrap_or(1))
}

fn print_digest(
    subcommand: &str,
    ordered_messages: &[DeduppedMessage],
    stderr_captured: &[String],
    success: bool,
) {
    let mut error_count = 0;
    let mut warning_count = 0;
    for msg in ordered_messages {
        if msg.level == "error" {
            error_count += 1;
        } else if msg.level == "warning" {
            warning_count += 1;
        }
    }

    let status_str = if success && error_count == 0 { "succeeded" } else { "failed" };
    println!("Cargo {} {}: {} errors, {} warnings", subcommand, status_str, error_count, warning_count);

    if !ordered_messages.is_empty() {
        println!();
        for msg in ordered_messages {
            let level_upper = msg.level.to_uppercase();
            if msg.line > 0 && !msg.file_name.is_empty() {
                println!("[{}] {}:{}: {}", level_upper, msg.file_name, msg.line, msg.message);
            } else {
                println!("[{}] {}", level_upper, msg.message);
            }
        }
    } else if !success {
        eprintln!();
        for line in stderr_captured {
            eprintln!("{}", line);
        }
    }
}

async fn read_streams(
    child: &mut tokio::process::Child,
    stdout: tokio::process::ChildStdout,
    stderr: tokio::process::ChildStderr,
    store: &Store,
    task_id: &Option<String>,
    progress_interval_ms: u64,
) -> Result<(Vec<DeduppedMessage>, Vec<String>, bool)> {
    let mut stdout_lines = BufReader::new(stdout).lines();
    let mut stderr_lines = BufReader::new(stderr).lines();

    let start = std::time::Instant::now();
    let mut last_progress = std::time::Instant::now();

    let mut dedupped = HashSet::new();
    let mut ordered_messages = Vec::new();
    let mut stderr_captured = Vec::new();

    loop {
        tokio::select! {
            line_res = stdout_lines.next_line() => {
                if let Ok(Some(line)) = line_res {
                    parse_stdout_line(&line, store, task_id, &mut dedupped, &mut ordered_messages);
                }
            }
            line_res = stderr_lines.next_line() => {
                if let Ok(Some(line)) = line_res {
                    stderr_captured.push(line);
                }
            }
            _ = tokio::time::sleep(tokio::time::Duration::from_millis(50)) => {
                let elapsed = start.elapsed();
                if elapsed.as_millis() >= 100 {
                    let last_elapsed_ms = last_progress.elapsed().as_millis();
                    if last_elapsed_ms >= progress_interval_ms as u128 {
                        eprintln!("[aid] cargo build: still running... (elapsed: {}s)", elapsed.as_secs());
                        last_progress = std::time::Instant::now();
                    }
                }
            }
        }

        if let Ok(Some(status)) = child.try_wait() {
            while let Ok(Some(line)) = stdout_lines.next_line().await {
                parse_stdout_line(&line, store, task_id, &mut dedupped, &mut ordered_messages);
            }
            while let Ok(Some(line)) = stderr_lines.next_line().await {
                stderr_captured.push(line);
            }
            return Ok((ordered_messages, stderr_captured, status.success()));
        }
    }
}

fn parse_stdout_line(
    line: &str,
    store: &Store,
    task_id: &Option<String>,
    dedupped: &mut HashSet<DeduppedMessage>,
    ordered_messages: &mut Vec<DeduppedMessage>,
) {
    if let Ok(val) = serde_json::from_str::<serde_json::Value>(line) {
        let reason = val.get("reason").and_then(|r| r.as_str());
        if reason == Some("compiler-message") {
            if let Ok(msg_reason) = serde_json::from_value::<CompilerMessageReason>(val) {
                let msg = msg_reason.message;
                let level = msg.level;
                if level == "error" || level == "warning" {
                    let primary_span = msg.spans.iter().find(|s| s.is_primary).or_else(|| msg.spans.first());
                    let (file_name, line_num) = if let Some(span) = primary_span {
                        (span.file_name.clone(), span.line_start)
                    } else {
                        ("".to_string(), 0)
                    };
                    let dedupped_msg = DeduppedMessage {
                        level,
                        file_name,
                        line: line_num,
                        message: msg.message,
                    };
                    if dedupped.insert(dedupped_msg.clone()) {
                        log_build_event(store, task_id, &dedupped_msg);
                        ordered_messages.push(dedupped_msg);
                    }
                }
            }
        } else if reason == Some("compiler-artifact") {
            if let Some(pkg_id) = val.get("package_id").and_then(|p| p.as_str()) {
                let pkg_name = pkg_id.split_whitespace().next().unwrap_or(pkg_id);
                if let Some(tid) = task_id.as_ref() {
                    let _ = store.insert_event(&crate::types::TaskEvent {
                        task_id: crate::types::TaskId(tid.clone()),
                        timestamp: chrono::Local::now(),
                        event_kind: crate::types::EventKind::Build,
                        detail: format!("Compiled {}", pkg_name),
                        metadata: None,
                    });
                }
            }
        }
    }
}

fn log_build_event(store: &Store, task_id: &Option<String>, msg: &DeduppedMessage) {
    if let Some(tid) = task_id.as_ref() {
        let detail = format!(
            "cargo {}: {} at {}:{}",
            msg.level, msg.message, msg.file_name, msg.line
        );
        let _ = store.insert_event(&crate::types::TaskEvent {
            task_id: crate::types::TaskId(tid.clone()),
            timestamp: chrono::Local::now(),
            event_kind: crate::types::EventKind::Build,
            detail,
            metadata: None,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_resolve_cargo_args_defaults() {
        let (sub, args) = resolve_cargo_args(&vec![]).unwrap();
        assert_eq!(sub, "check");
        assert!(args.contains(&"--message-format=json".to_string()));
    }

    #[test]
    fn test_resolve_cargo_args_with_cmd() {
        let (sub, args) = resolve_cargo_args(&vec!["build".to_string(), "--release".to_string()]).unwrap();
        assert_eq!(sub, "build");
        assert_eq!(args[0], "build");
        assert_eq!(args[1], "--message-format=json");
        assert_eq!(args[2], "--release");
    }

    #[test]
    fn test_resolve_cargo_args_with_dashdash() {
        let (sub, args) = resolve_cargo_args(&vec![
            "test".to_string(),
            "--".to_string(),
            "--test-threads=1".to_string(),
        ])
        .unwrap();
        assert_eq!(sub, "test");
        assert_eq!(args[0], "test");
        assert_eq!(args[1], "--message-format=json");
        assert_eq!(args[2], "--");
        assert_eq!(args[3], "--test-threads=1");
    }
}
