// Handler for `aid ask` plus a silent text-returning helper for MCP.
// Exports run() for CLI and ask_text() for programmatic quick research.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::cmd::show;
use crate::store::Store;
use crate::types::TaskId;

pub async fn run(
    store: Arc<Store>,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<()> {
    let request = prepare_request(prompt, agent, model, files, output)?;
    announce_context_files(&request.context_files);
    let _ = dispatch(store, request, true).await?;
    Ok(())
}

pub async fn ask_text(
    store: Arc<Store>,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
) -> Result<String> {
    let capture_path = temp_output_path();
    let request = prepare_request(
        prompt,
        agent,
        model,
        vec![],
        Some(capture_path.display().to_string()),
    )?;
    let task_id = dispatch(store.clone(), request, false).await?;
    let answer = read_answer(&task_id, &capture_path)?;
    let _ = std::fs::remove_file(&capture_path);
    Ok(answer)
}

struct AskRequest {
    agent_name: String,
    prompt: String,
    model: Option<String>,
    output: Option<String>,
    context_files: Vec<String>,
}

fn prepare_request(
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<AskRequest> {
    let context_files = detect_context_files(&prompt, files);
    let prompt = inject_context(prompt, &context_files)?;
    Ok(AskRequest {
        agent_name: agent.unwrap_or_else(|| "gemini".to_string()),
        prompt,
        model,
        output,
        context_files,
    })
}

fn detect_context_files(prompt: &str, files: Vec<String>) -> Vec<String> {
    if files.is_empty() {
        crate::explore::auto_detect_files(prompt, Path::new("."))
    } else {
        files
    }
}

fn inject_context(prompt: String, context_files: &[String]) -> Result<String> {
    if context_files.is_empty() {
        return Ok(prompt);
    }
    let specs = crate::context::parse_context_specs(context_files)?;
    let context = crate::context::resolve_context(&specs)?;
    Ok(crate::context::inject_context(&prompt, &context))
}

fn announce_context_files(context_files: &[String]) {
    if context_files.is_empty() {
        println!("[ask] Using files: (none)");
    } else {
        println!("[ask] Using files: {}", context_files.join(", "));
    }
}

async fn dispatch(store: Arc<Store>, request: AskRequest, announce: bool) -> Result<TaskId> {
    run::run(
        store,
        RunArgs {
            agent_name: request.agent_name,
            prompt: request.prompt,
            repo: None,
            dir: None,
            output: request.output,
            model: request.model,
            worktree: None,
            base_branch: None,
            group: None,
            verify: None,
            max_duration_mins: None,
            retry: 0,
            context: vec![],
            skills: vec![],
            template: None,
            background: false,
            announce,
            parent_task_id: None,
            on_done: None,
            fallback: None,
            read_only: false,
            session_id: None,
        },
    )
    .await
}

fn temp_output_path() -> PathBuf {
    std::env::temp_dir().join(format!("aid-ask-{}.txt", TaskId::generate()))
}

fn read_answer(task_id: &TaskId, capture_path: &Path) -> Result<String> {
    if capture_path.exists() {
        return Ok(std::fs::read_to_string(capture_path)?);
    }
    show::log_text(task_id.as_str())
}
