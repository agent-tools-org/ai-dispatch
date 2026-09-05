// CLI error diagnostics independent of task creation and database availability.
// Exports parsing, error recording/history, and shared run-option validation.
// Deps: clap, anyhow, serde, local history/parser/validation modules.

mod history;
mod parsing;
mod run_options;

pub(crate) use history::{ErrorsArgs, show};
pub(crate) use parsing::parse;
pub(crate) use run_options::validate_run_options;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct Issue {
    pub code: String,
    pub message: String,
    pub hint: String,
}

impl Issue {
    pub fn new(code: &str, message: &str, hint: &str) -> Self {
        Self { code: code.into(), message: message.into(), hint: hint.into() }
    }
}

#[derive(Debug)]
pub(crate) struct Rejection(pub Vec<Issue>);

impl std::fmt::Display for Rejection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "Invalid run options ({} issues):", self.0.len())?;
        for issue in &self.0 {
            writeln!(f, "  [{}] {}\n    {}", issue.code, issue.message, issue.hint)?;
        }
        Ok(())
    }
}

impl std::error::Error for Rejection {}

pub(crate) fn record_error(error: &anyhow::Error) {
    if let Some(rejection) = error.downcast_ref::<Rejection>() {
        history::record("validation", 1, rejection.0.clone());
        return;
    }
    // Keep unstructured failures out of the rejection category. They may be
    // runtime/I/O failures; their messages can contain config or provider secrets.
    history::record("command", 1, vec![Issue::new(
        "CommandFailed", "Command returned an error.",
        "See the command's stderr and, if a task exists, aid show <task-id> --events.",
    )]);
}
