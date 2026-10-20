// Session reuse and clearing across manual retry routes.
// Shares retry fixtures from retry_saved_args_tests.
use super::*;

#[test]
fn retry_preservation_manual_retry_of_retry_keeps_all_feedback() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-manual-retry-preservation");
    task.prompt =
        "[Previous attempt failed]\nExecution error\n\n[Original task]\nChild brief".repeat(200);
    task.resolved_prompt = Some("Injected launch context".into());
    store.insert_task(&task).unwrap();
    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("New correction".into()),
            feedback_file: None,
            agent: None,
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap();
    assert_eq!(
        args.prompt,
        format!(
            "[Previous attempt feedback]\nNew correction\n\n[Original task]\n{}",
            task.prompt,
        )
    );
    assert!(!args.prompt.contains("Injected launch context"));
}


#[test]
fn retry_preservation_keeps_long_feedback_first_and_whole_on_codex_resume() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-feedback-resume");
    task.agent_session_id = Some("session-feedback".into());
    task.prompt = "original task line\n".repeat(1000);
    store.insert_task(&task).unwrap();
    let saved = RunArgs {
        context: vec!["context.md".into()],
        ..Default::default()
    };
    store
        .update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().unwrap())
        .unwrap();
    let feedback = "fix every missing requirement\n".repeat(300);
    let retry = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some(feedback.clone()),
            feedback_file: None,
            agent: None,
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap();
    assert_eq!(retry.context, saved.context);
    assert_eq!(retry.session_id, task.agent_session_id);
    assert!(
        retry
            .prompt
            .starts_with(&format!("[Previous attempt feedback]\n{feedback}"))
    );
    let composed =
        crate::task::workgroup::compose_prompt(&retry.prompt, Some(&"context\n".repeat(1000)), None, &[], &[]);
    let delivered = codex_resume_prompt(&composed.text, &retry);
    assert!(delivered.contains(&retry.prompt));
    assert!(delivered.find(&feedback) < delivered.find(&task.prompt));
}

fn codex_resume_prompt(prompt: &str, retry: &RunArgs) -> String {
    use crate::agent::{Agent, CommandContext, RunOpts};
    let opts = RunOpts {
        dir: None,
        output: None,
        result_file: None,
        model: None,
        budget: false,
        read_only: false,
        sandbox: false,
        context_files: vec![],
        session_id: retry.session_id.clone(),
        env: None,
        env_forward: None,
    };
    let command = crate::agent::codex::CodexAgent
        .build_command_with_context(
            prompt,
            &opts,
            CommandContext {
                durable_codex_home: false,
                writable_roots: vec![],
            },
        )
        .unwrap();
    let argv: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy()).collect();
    assert_eq!(
        &argv[..5],
        [
            "exec",
            "resume",
            "--json",
            "--skip-git-repo-check",
            "session-feedback"
        ]
    );
    argv[5].to_string()
}

/// `aid retry <id> --agent <other>` must drop both model AND session_id.
/// Before this fix, session_id was set based on whether the OLD agent supports
/// session resume — it was never cleared on agent change.
#[test]
fn retry_agent_switch_clears_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-switch");
    // Codex supports session resume; the session is recorded at task
    // completion and stored in agent_session_id.
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-abc123".to_string());
    task.requested_model = Some("gpt-5".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_task_to_run_args(
        &store,
        &task,
        RetryArgs {
            task_id: task.id.to_string(),
            feedback: Some("fix the bug".to_string()),
            feedback_file: None,
            agent: Some("grok".to_string()), // switching to a different CLI
            model: None,
            idle_timeout_secs: None,
            dir: None,
            reset: false,
            bg: false,
        },
        false,
    )
    .unwrap();

    assert_eq!(args.agent_name, "grok");
    assert!(
        args.session_id.is_none(),
        "session_id must be cleared when agent changes, got {:?}",
        args.session_id
    );
    // model must also be cleared (it is route-owned, not task-owned)
    assert!(args.model.is_none(), "model must be cleared when agent changes");
}

/// A same-agent retry must still resume its session.
#[test]
fn retry_same_agent_preserves_session_id() {
    let store = Store::open_memory().unwrap();
    let mut task = failed_task("t-session-same");
    task.agent = AgentKind::Codex;
    task.agent_session_id = Some("session-xyz789".to_string());
    store.insert_task(&task).unwrap();

    let args = retry_args(&store, &task, None); // no --agent override → same agent

    assert_eq!(args.agent_name, "codex");
    assert_eq!(
        args.session_id.as_deref(),
        Some("session-xyz789"),
        "same-agent retry must preserve the session id"
    );
}
