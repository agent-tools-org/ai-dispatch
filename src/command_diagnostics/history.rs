// Append-only, task-independent CLI error history with redacted arguments.
// Exports ErrorsArgs, record(), show(); depends on paths, clap, serde_json, std I/O.

use std::collections::{HashSet, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use anyhow::Result;
use clap::{Args, CommandFactory};
use serde::{Deserialize, Serialize};
use super::Issue;

const LOG_NAME: &str = "command-errors.jsonl";

#[derive(Args)]
pub(crate) struct ErrorsArgs {
    /// Maximum number of recent CLI errors to show, newest first
    #[arg(long, default_value = "20", value_parser = clap::value_parser!(u32).range(1..=1000))]
    pub limit: u32,
    /// Print structured records, including stable validation issue codes
    #[arg(long)]
    pub json: bool,
}

#[derive(Serialize, Deserialize)]
struct Record {
    timestamp: String,
    pid: u32,
    cwd: Option<std::path::PathBuf>,
    argv: Vec<String>,
    stage: String,
    exit_code: i32,
    issues: Vec<Issue>,
}

pub(super) fn record(stage: &str, exit_code: i32, issues: Vec<Issue>) {
    let record = Record {
        timestamp: chrono::Utc::now().to_rfc3339(),
        pid: std::process::id(),
        cwd: std::env::current_dir().ok(),
        argv: redacted_argv(),
        stage: stage.into(), exit_code, issues,
    };
    if let Err(error) = append(&record) {
        eprintln!("[aid] Could not record command error: {error}");
    }
}

fn append(record: &Record) -> Result<()> {
    let mut bytes = serde_json::to_vec(record)?;
    bytes.push(b'\n');
    let path = crate::paths::logs_dir().join(LOG_NAME);
    std::fs::create_dir_all(crate::paths::logs_dir())?;
    let mut options = OpenOptions::new();
    options.create(true).append(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(path)?;
    // Serialize concurrent process appends, including any short writes.
    file.lock()?;
    file.write_all(&bytes)?;
    Ok(())
}

fn redacted_argv() -> Vec<String> {
    let command = crate::cli::Cli::command();
    let mut flags = HashSet::new();
    collect_flags(&command, &mut flags);
    let mut found_command = false;
    std::env::args_os().skip(1).map(|arg| {
        let arg = arg.to_string_lossy();
        let flag = arg.split('=').next().unwrap_or_default();
        if flags.contains(flag) {
            return if arg.contains('=') { format!("{flag}=<redacted>") } else { flag.into() };
        }
        if !found_command && command.get_subcommands().any(|cmd| cmd.get_name() == arg) {
            found_command = true;
            return arg.into_owned();
        }
        "<redacted>".into()
    }).collect()
}

fn collect_flags(command: &clap::Command, flags: &mut HashSet<String>) {
    for arg in command.get_arguments() {
        if let Some(long) = arg.get_long() { flags.insert(format!("--{long}")); }
        if let Some(short) = arg.get_short() { flags.insert(format!("-{short}")); }
    }
    for child in command.get_subcommands() { collect_flags(child, flags); }
}

pub(crate) fn show(args: &ErrorsArgs) -> Result<()> {
    let records = recent(args.limit as usize)?;
    if args.json {
        println!("{}", serde_json::to_string(&records)?);
        return Ok(());
    }
    if records.is_empty() { println!("No recorded command errors."); }
    for record in records {
        println!("{} [{}] exit {}: {}", record.timestamp, record.stage, record.exit_code, record.argv.join(" "));
        for issue in record.issues {
            println!("  [{}] {}\n    {}", issue.code, issue.message, issue.hint);
        }
    }
    Ok(())
}

fn recent(limit: usize) -> Result<Vec<Record>> {
    let file = match File::open(crate::paths::logs_dir().join(LOG_NAME)) {
        Ok(file) => file,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error.into()),
    };
    file.lock_shared()?;
    let mut records = VecDeque::with_capacity(limit);
    for line in BufReader::new(file).lines() {
        let line = line?;
        // A killed writer can leave a partial last record; preserve readable history.
        let Ok(record) = serde_json::from_str::<Record>(&line) else { continue };
        if records.len() == limit { records.pop_front(); }
        records.push_back(record);
    }
    Ok(records.into_iter().rev().collect())
}
