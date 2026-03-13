// Handler for `aid ask` — research/explore via cheap AI CLIs.
// Detects likely context files, injects them into the prompt, then dispatches.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::store::Store;

pub async fn run(
    store: Arc<Store>,
    prompt: String,
    agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<()> {
    let context_files = if files.is_empty() {
        crate::explore::auto_detect_files(&prompt, std::path::Path::new("."))
    } else {
        files
    };
    let agent_name = agent.unwrap_or_else(|| "gemini".to_string());
    if context_files.is_empty() {
        println!("[ask] Using files: (none)");
    } else {
        println!("[ask] Using files: {}", context_files.join(", "));
    }
    let effective_prompt = if context_files.is_empty() {
        prompt
    } else {
        let specs = crate::context::parse_context_specs(&context_files)?;
        let context = crate::context::resolve_context(&specs)?;
        crate::context::inject_context(&prompt, &context)
    };

    let _ = run::run(
        store,
        RunArgs {
            agent_name,
            prompt: effective_prompt,
            dir: None,
            output,
            model,
            worktree: None,
            group: None,
            verify: None,
            retry: 0,
            context: vec![],
            background: false,
            parent_task_id: None,
        },
    )
    .await?;
    Ok(())
}
