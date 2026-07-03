// Handler for `aid ask` plus a silent text-returning helper for MCP.
// Exports run() for CLI and ask_text() for programmatic quick research.

use anyhow::Result;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::cmd::show;
use crate::agent_config;
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
    let agent_name = match agent {
        Some(agent) => agent,
        None => {
            if agent_config::is_agent_disabled("gemini") {
                anyhow::bail!("Default agent 'gemini' is disabled (choose another with: aid ask --agent <name>)");
            }
            "gemini".to_string()
        }
    };
    Ok(AskRequest {
        agent_name,
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
            output: request.output,
            model: request.model,
            announce,
            ..Default::default()
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;

    #[test]
    fn prepare_request_rejects_disabled_default_agent() {
        let dir = tempfile::tempdir().expect("tempdir");
        let _guard = AidHomeGuard::set(dir.path());
        agent_config::save_agent_disabled("gemini", true).expect("disable agent");

        let err = match prepare_request(
            "Explain this".to_string(),
            None,
            None,
            vec![],
            None,
        ) {
            Ok(_) => panic!("disabled default should fail"),
            Err(err) => err.to_string(),
        };

        assert_eq!(
            err,
            "Default agent 'gemini' is disabled (choose another with: aid ask --agent <name>)"
        );
    }
}
