// Handler for `aid explore` foundation wiring.
// Delegates to `aid run` using Gemini until custom explore logic lands.

use anyhow::Result;
use std::sync::Arc;

use crate::cmd::run::{self, RunArgs};
use crate::store::Store;

pub async fn run(
    store: Arc<Store>,
    prompt: String,
    _agent: Option<String>,
    model: Option<String>,
    files: Vec<String>,
    output: Option<String>,
) -> Result<()> {
    let context = if files.is_empty() {
        crate::explore::auto_detect_files(&prompt, std::path::Path::new("."))
    } else {
        files
    };

    run::run(
        store,
        RunArgs {
            agent_name: "gemini".to_string(),
            prompt,
            dir: None,
            output,
            model,
            worktree: None,
            verify: None,
            context,
            background: false,
        },
    )
    .await
}
