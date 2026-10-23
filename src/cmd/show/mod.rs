// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::cmd;
use crate::store::Store;
use crate::types::{Task, TaskStatus};
use crate::ui::board::render_task_detail;

#[path = "output.rs"]
mod show_output;

#[allow(unused_imports)]
pub use show_output::read_task_output;
pub(crate) use show_output::{
    branch_base_ref, diff_stat, diff_text_branch, diff_text_file, extract_messages_from_log,
    is_non_output_line, parse_diff_stat, read_tail, worktree_diff, worktree_state_section,
};
pub use show_output::{
    diff_text, log_text, log_text_brief, output_text, output_text_brief, output_text_for_task,
};
pub(crate) use show_output::{missing_owned_output_absence, owned_output_path};

mod text;
use text::*;
pub use text::{context_text, summary_text};

#[path = "helpers.rs"]
mod show_helpers;
#[path = "json.rs"]
mod show_json;

pub(crate) use show_helpers::load_task;
pub(crate) use show_json::task_hook_json;

use show_helpers::{
    completion_conclusion, failure_details, inplace_diff_stat, research_findings, stderr_tail,
    task_has_changes,
};
use show_json::task_json;

pub struct ShowArgs {
    pub task_id: String,
    pub events: bool,
    pub context: bool,
    pub diff: bool,
    pub summary: bool,
    pub file: Option<String>,
    pub branch: bool,
    pub output: bool,
    pub result: bool,
    pub transcript: bool,
    pub full: bool,
    pub brief: bool,
    pub explain: bool,
    pub log: bool,
    pub json: bool,
    pub agent: Option<String>,
    pub model: Option<String>,
}

#[derive(Clone, Copy)]
pub enum ShowMode {
    Default,
    Summary,
    Events,
    Context,
    Diff,
    Output,
    Transcript,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
    if args.result {
        print!("{}", result_text(&store, &args.task_id)?);
        return Ok(());
    }
    if args.json {
        let text = task_json(&store, &args.task_id)?;
        println!("{text}");
        return Ok(());
    }
    if args.context {
        let text = render_mode_text(&store, &args.task_id, ShowMode::Context)?;
        print!("{text}");
        return Ok(());
    }
    if args.explain {
        return cmd::explain::run_explain(store, &args.task_id, args.agent, args.model).await;
    }
    let mode = selected_mode(&args);
    let task = load_task(&store, &args.task_id)?;
    let text = render_selected_mode(&store, &args, mode)?;
    print!("{text}");
    if matches!(mode, ShowMode::Diff) {
        aid_hint!(
            "[aid] Actions: aid merge {} | aid retry {} -f \"feedback\"",
            args.task_id,
            args.task_id
        );
    }
    if !task.status.is_terminal() {
        aid_hint!(
            "[aid] Task is still running. To wait for completion: aid wait {}",
            args.task_id
        );
    }
    Ok(())
}

pub fn render_mode_text(store: &Arc<Store>, task_id: &str, mode: ShowMode) -> Result<String> {
    match mode {
        ShowMode::Default => audit_text(store, task_id),
        ShowMode::Summary => summary_text(store, task_id),
        ShowMode::Events => events_text(store, task_id, false),
        ShowMode::Context => context_text(store, task_id),
        ShowMode::Diff => diff_text(store, task_id),
        ShowMode::Output => output_text(store, task_id),
        ShowMode::Transcript => transcript_text(task_id),
        ShowMode::Log => log_text(task_id),
    }
}

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let retry_chain = if task.parent_task_id.is_some() {
        Some(store.get_retry_chain(task_id)?)
    } else {
        None
    };
    let mut out = render_task_detail(&task, &events, retry_chain);
    append_audit_details(store, task_id, &task, &events, &mut out)?;
    append_audit_changes(&mut out, &task);

    if let Some(banner) = audit_result_missing_banner(store, &task) {
        out.push('\n');
        out.push_str(&banner);
    } else if !task_has_changes(&task)
        && task.status.is_terminal()
        && let Some(findings) = research_findings(store.as_ref(), &task)
    {
        out.push_str("\nFindings:\n");
        out.push_str(&findings);
        out.push('\n');
        aid_hint!(
            "[aid] Research task. Full output: aid show {} --output --full",
            task.id
        );
    }

    Ok(out)
}

#[cfg(test)]
mod tests;

#[cfg(test)]
#[path = "mode_tests.rs"]
mod show_mode_tests;

#[cfg(test)]
#[path = "banner_tests.rs"]
mod banner_tests;
#[cfg(test)]
#[path = "checklist_tests.rs"]
mod show_checklist_tests;

fn selected_mode(args: &ShowArgs) -> ShowMode {
    if args.summary {
        ShowMode::Summary
    } else if args.events {
        ShowMode::Events
    } else if args.diff {
        ShowMode::Diff
    } else if args.output {
        ShowMode::Output
    } else if args.transcript {
        ShowMode::Transcript
    } else if args.log {
        ShowMode::Log
    } else {
        ShowMode::Default
    }
}

fn render_selected_mode(store: &Arc<Store>, args: &ShowArgs, mode: ShowMode) -> Result<String> {
    Ok(if matches!(mode, ShowMode::Events) {
        events_text(store, &args.task_id, args.full)?
    } else if matches!(mode, ShowMode::Output) {
        render_output_text(store, &args.task_id, args.full, args.brief)?
    } else if matches!(mode, ShowMode::Log) {
        render_log_text(&args.task_id, args.full)?
    } else if matches!(mode, ShowMode::Diff) {
        if let Some(file) = args.file.as_deref() {
            diff_text_file(store, &args.task_id, file, args.branch)?
        } else if args.branch {
            diff_text_branch(store, &args.task_id)?
        } else {
            diff_text(store, &args.task_id)?
        }
    } else {
        render_mode_text(store, &args.task_id, mode)?
    })
}

fn append_audit_changes(out: &mut String, task: &Task) {
    let mut live_dirty = false;
    if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        let state =
            crate::workspace::worktree::capture_live_worktree_state(Path::new(wt_path)).ok();
        live_dirty = state.as_ref().is_some_and(|state| state.is_dirty());
        out.push_str(&worktree_state_section(wt_path, state.as_ref()));
    }

    if task
        .delivery_assessment()
        .is_some_and(|delivery| delivery.implies_no_changes())
        && !live_dirty
    {
        out.push_str("\nChanges:\n[no changes]\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        let base_ref = branch_base_ref(wt_path);
        let stat = diff_stat(wt_path, task.start_sha.as_deref(), base_ref.as_deref());
        if live_dirty && stat.contains("(no changes detected)") {
            out.push_str("  (no committed diff detected)\n");
        } else {
            out.push_str(&stat);
        }
    } else if task.worktree_branch.is_none()
        && matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
    {
        // In-place task: show working tree diff stat from repo
        let repo = task.repo_path.as_deref().unwrap_or(".");
        if let Some(stat) = inplace_diff_stat(repo) {
            out.push_str("\nWorking tree changes (in-place edit):\n");
            out.push_str(&stat);
        }
    }
}

fn append_audit_details(
    store: &Store,
    task_id: &str,
    task: &Task,
    events: &[crate::types::TaskEvent],
    out: &mut String,
) -> Result<()> {
    if let Some(override_reason) = store.get_principal_merge_override(task_id)? {
        out.push_str(&format!("Principal merge override: {override_reason}\n"));
    }
    if let Some(url) = store.backup_url(task_id)? {
        out.push_str(&format!("Backup: {url}\n"));
    }
    if let Some(notice) = terminal_missing_result_notice(task) {
        out.push('\n');
        out.push_str(&notice);
    }

    if let Some(checklist) = cmd::show_checklist::render_checklist_status(store, task) {
        out.push('\n');
        out.push_str(&checklist);
    }

    if task.status == TaskStatus::Failed
        && let Some(details) = failure_details(events)
    {
        out.push_str("\nFailure details:\n");
        out.push_str(&details);
    }

    if task.status == TaskStatus::Failed
        && let Some(stderr) = stderr_tail(task_id)
    {
        out.push_str("\nStderr:\n");
        out.push_str(&stderr);
    }

    Ok(())
}
