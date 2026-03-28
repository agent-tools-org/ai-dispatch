// Handler for `aid show <task-id>` — unified task inspection.
// Combines events, diff, output, log, and AI explanation into one command.

use anyhow::{Context, Result};
use std::path::Path;
use std::sync::Arc;

use crate::board::render_task_detail;
use crate::cmd;
use crate::store::Store;
use crate::types::{TaskStatus, VerifyStatus};

#[path = "show_output.rs"]
mod show_output;

pub use show_output::{
    diff_text, log_text, output_text, output_text_brief, output_text_for_task,
};
#[allow(unused_imports)]
pub use show_output::output_text_full;
#[allow(unused_imports)]
pub use show_output::read_task_output;
pub(crate) use show_output::{
    diff_stat, diff_text_file, extract_messages_from_log, parse_diff_stat, read_tail,
    worktree_diff,
};

#[path = "show_helpers.rs"]
mod show_helpers;
#[path = "show_json.rs"]
mod show_json;

pub(crate) use show_helpers::load_task;
pub(crate) use show_json::task_hook_json;

use show_json::task_json;
use show_helpers::{
    completion_conclusion, inplace_diff_stat, reconstruct_context, research_findings,
    stderr_tail, task_has_changes,
};

pub struct ShowArgs {
    pub task_id: String,
    pub context: bool,
    pub diff: bool,
    pub summary: bool,
    pub file: Option<String>,
    pub output: bool,
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
    Summary,
    StatOnly,
    Context,
    Diff,
    Output,
    Transcript,
    Log,
}

pub async fn run(store: Arc<Store>, args: ShowArgs) -> Result<()> {
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
    let mode = if args.summary {
        ShowMode::StatOnly
    } else if args.diff {
        ShowMode::Diff
    } else if args.output {
        ShowMode::Output
    } else if args.transcript {
        ShowMode::Transcript
    } else if args.log {
        ShowMode::Log
    } else {
        ShowMode::Summary
    };
    let _ = args.full;
    let task = load_task(&store, &args.task_id)?;
    let text = if matches!(mode, ShowMode::Output) && args.brief {
        render_output_brief_text(&store, &args.task_id)?
    } else if matches!(mode, ShowMode::Diff) {
        if let Some(file) = args.file.as_deref() {
            diff_text_file(&store, &args.task_id, file)?
        } else {
            diff_text(&store, &args.task_id)?
        }
    } else {
        render_mode_text(&store, &args.task_id, mode)?
    };
    print!("{text}");
    if matches!(mode, ShowMode::Diff) {
        aid_hint!(
            "[aid] Actions: aid merge {} | aid retry {} -f \"feedback\"",
            args.task_id, args.task_id
        );
    }
    if !task.status.is_terminal() {
        aid_hint!(
            "[aid] Task is still running. To wait for completion: aid watch --quiet {}",
            args.task_id
        );
    }
    Ok(())
}

fn render_output_brief_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let mut text = output_text_brief(store, task_id)?;
    let truncated = output_text(store, task_id)
        .map(|full_text| full_text != text)
        .unwrap_or(false);
    if truncated {
        if !text.ends_with('\n') {
            text.push('\n');
        }
        text.push_str(&format!(
            "[truncated — use `aid show {task_id} --output` for full content]"
        ));
    }
    Ok(text)
}

pub fn render_mode_text(store: &Arc<Store>, task_id: &str, mode: ShowMode) -> Result<String> {
    match mode {
        ShowMode::Summary => audit_text(store, task_id),
        ShowMode::StatOnly => summary_text(store, task_id),
        ShowMode::Context => context_text(store, task_id),
        ShowMode::Diff => diff_text(store, task_id),
        ShowMode::Output => output_text(store, task_id),
        ShowMode::Transcript => transcript_text(task_id),
        ShowMode::Log => log_text(task_id),
    }
}

fn transcript_text(task_id: &str) -> Result<String> {
    let path = crate::paths::transcript_path(task_id);
    std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read transcript {}", path.display()))
}

// --- Default mode: events + stderr + diff stat ---

pub fn audit_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let events = store.get_events(task_id)?;
    let retry_chain = if task.parent_task_id.is_some() {
        Some(store.get_retry_chain(task_id)?)
    } else {
        None
    };
    let mut out = render_task_detail(&task, &events, retry_chain);

    if let Some(checklist) = cmd::show_checklist::render_checklist_status(store.as_ref(), &task) {
        out.push('\n');
        out.push_str(&checklist);
    }

    if task.status == TaskStatus::Failed
        && let Some(stderr) = stderr_tail(task_id)
    {
        out.push_str("\nStderr:\n");
        out.push_str(&stderr);
    }

    if task.verify_status == VerifyStatus::EmptyDiff {
        out.push_str("\nChanges:\n[no changes]\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\nChanges:\n");
        out.push_str(&diff_stat(wt_path));
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

    if !task_has_changes(&task) && task.status.is_terminal()
        && let Some(findings) = research_findings(store.as_ref(), &task)
    {
        out.push_str("\nFindings:\n");
        out.push_str(&findings);
        out.push('\n');
        aid_hint!(
            "[aid] Research task. Full output: aid show {} --output",
            task.id
        );
    }

    Ok(out)
}

pub fn summary_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str(&format!("=== Review: {} ===\n", task.id));
    out.push_str(&format!(
        "Agent: {}  Status: {}  Prompt: {}\n",
        task.agent_display_name(),
        task.status.label(),
        task.prompt,
    ));

    if task.verify_status == VerifyStatus::EmptyDiff {
        out.push_str("\n--- Diff Stat ---\n  (no changes detected)\n");
    } else if let Some(ref wt_path) = task.worktree_path
        && Path::new(wt_path).exists()
    {
        out.push_str("\n--- Diff Stat ---\n");
        out.push_str(&diff_stat(wt_path));
    } else if task.worktree_branch.is_none()
        && matches!(task.status, TaskStatus::Done | TaskStatus::Merged)
    {
        let repo = task.repo_path.as_deref().unwrap_or(".");
        out.push_str("\n--- Diff Stat ---\n");
        if let Some(stat) = inplace_diff_stat(repo) {
            out.push_str(&stat);
        } else {
            out.push_str("  (no changes detected)\n");
        }
    }

    if !out.contains("--- Diff Stat ---") || out.contains("(no changes detected)") {
        if let Some(conclusion) = completion_conclusion(store.as_ref(), task.id.as_str()) {
            out.push_str("\nConclusion: ");
            out.push_str(&conclusion);
            out.push('\n');
        }
    }

    Ok(out)
}

pub fn context_text(store: &Arc<Store>, task_id: &str) -> Result<String> {
    let task = load_task(store, task_id)?;
    let mut out = String::new();
    out.push_str("=== Original Prompt ===\n");
    out.push_str(&task.prompt);
    out.push('\n');

    if let Some(resolved_prompt) = task.resolved_prompt.as_deref() {
        out.push_str("\n=== Resolved Prompt ===\n");
        out.push_str(resolved_prompt);
        out.push('\n');
        return Ok(out);
    }

    let (skill_content, resolved_prompt) = reconstruct_context(store, &task)?;
    out.push_str("\n(reconstructed — context files may have changed since dispatch)\n");
    out.push_str("\n=== Injected Skills ===\n");
    if skill_content.trim().is_empty() {
        out.push_str("(none)\n");
    } else {
        out.push_str(&skill_content);
        out.push('\n');
    }
    out.push_str("\n=== Resolved Prompt ===\n");
    out.push_str(&resolved_prompt);
    out.push('\n');
    Ok(out)
}

#[cfg(test)]
#[path = "show_tests.rs"]
mod tests;

#[cfg(test)]
#[path = "show_checklist_tests.rs"]
mod show_checklist_tests;
