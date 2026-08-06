// Tests for pre-dispatch model validation.

use super::*;
use std::process::Command;
use crate::agent::{Agent, RunOpts};
use crate::types::*;

struct MockQueryableAgent {
    kind: AgentKind,
    models: Option<Vec<String>>,
}

impl Agent for MockQueryableAgent {
    fn kind(&self) -> AgentKind {
        self.kind
    }
    fn streaming(&self) -> bool {
        false
    }
    fn build_command(&self, _prompt: &str, _opts: &RunOpts) -> Result<Command> {
        Ok(Command::new("true"))
    }
    fn parse_event(&self, _task_id: &TaskId, _line: &str) -> Option<TaskEvent> {
        None
    }
    fn served_models(&self) -> Result<Option<Vec<String>>> {
        Ok(self.models.clone())
    }
}

#[test]
fn validate_model_allows_valid_model() {
    clear_served_models_cache();
    let mock = MockQueryableAgent {
        kind: AgentKind::Codex,
        models: Some(vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()]),
    };

    assert!(validate_model_for_agent(&mock, "gpt-5.6-sol").is_ok());
    assert!(validate_model_for_agent(&mock, "GPT-5.5").is_ok());
}

#[test]
fn validate_model_rejects_absent_model_naming_served() {
    clear_served_models_cache();
    let mock = MockQueryableAgent {
        kind: AgentKind::Codex,
        models: Some(vec!["gpt-5.6-sol".to_string(), "gpt-5.5".to_string()]),
    };

    let err = validate_model_for_agent(&mock, "auto").unwrap_err().to_string();
    assert!(err.contains("Agent 'codex' does not serve model 'auto'"));
    assert!(err.contains("Served models: gpt-5.6-sol, gpt-5.5"));
}

#[test]
fn validate_model_allows_unqueryable_cli() {
    clear_served_models_cache();
    let mock = MockQueryableAgent {
        kind: AgentKind::Kilo,
        models: None,
    };

    assert!(validate_model_for_agent(&mock, "any-unknown-model").is_ok());
}

#[test]
fn cursor_auto_model_is_allowed() {
    clear_served_models_cache();
    let mock = MockQueryableAgent {
        kind: AgentKind::Cursor,
        models: Some(vec![
            "composer-2.5".to_string(),
            "auto".to_string(),
            "default".to_string(),
        ]),
    };

    assert!(validate_model_for_agent(&mock, "auto").is_ok());
}
