// Handler for `aid respond` input forwarding.
// Writes one response payload for a background task under ~/.aid/jobs.

use anyhow::{Context, Result};

use crate::cmd::reply::{InputCommand, ensure_interactive_input};
use crate::runtime::input_signal;
use crate::store::Store;

pub fn run(store: &Store, task_id: &str, input: Option<&str>, file: Option<&str>) -> Result<()> {
    let task = store
        .get_task(task_id)?
        .ok_or_else(|| anyhow::anyhow!("Task {task_id} not found"))?;
    ensure_interactive_input(&task, InputCommand::Respond)?;
    let text = if let Some(path) = file {
        std::fs::read_to_string(path)
            .with_context(|| format!("Failed to read response file: {path}"))?
    } else if let Some(text) = input {
        text.to_string()
    } else {
        use std::io::Read;

        let mut buf = String::new();
        std::io::stdin()
            .read_to_string(&mut buf)
            .context("Failed to read from stdin")?;
        buf
    };
    input_signal::write_response(task_id, &text)?;
    println!("Queued input for {task_id}");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::run;
    use crate::runtime::input_signal;

    use crate::types::{AgentKind, Task, TaskId, TaskStatus};

    fn make_task(id: &str, agent: AgentKind) -> Task {
        Task { agent,
            prompt: "test".to_string(),
            ..crate::types::Task::test_fixture(TaskId(id.to_string()), TaskStatus::Running)
        }
    }

    #[test]
    fn respond_rejects_one_shot_agents_without_writing_signal() {
        let (_temp_home, store) = crate::support::test_env::TestHome::new().store();
        for (task_id, agent) in [("t-respond-agy", AgentKind::Antigravity), ("t-respond-grok", AgentKind::Grok)] {
            store.insert_task(&make_task(task_id, agent)).unwrap();
            let err = run(&store, task_id, Some("yes"), None).unwrap_err();
            assert!(err.to_string().contains("no response signal was written"));
            assert!(input_signal::take_response(task_id).unwrap().is_none());
        }
    }

    #[test]
    fn respond_refuses_codex_without_writing_signal() {
        let (_temp_home, store) = crate::support::test_env::TestHome::new().store();
        store.insert_task(&make_task("t-respond-codex", AgentKind::Codex)).unwrap();
        let error = run(&store, "t-respond-codex", Some("yes"), None).unwrap_err();
        assert!(error.to_string().contains("aid retry t-respond-codex -f"));
        assert!(input_signal::take_response("t-respond-codex").unwrap().is_none());
    }

    #[test]
    fn respond_explains_how_to_recover_deleted_custom_agent() {
        let (_temp_home, store) = crate::support::test_env::TestHome::new().store();
        let mut task = make_task("t-respond-missing-custom", AgentKind::Custom);
        task.custom_agent_name = Some("gone".to_string());
        store.insert_task(&task).unwrap();

        let err = run(&store, task.id.as_str(), Some("yes"), None).unwrap_err();
        let message = err.to_string();
        assert!(message.contains("unavailable custom agent 'gone'"));
        assert!(message.contains("restore ~/.aid/agents/gone.toml"));
        assert!(message.contains("stop the task and retry"));
    }
}
