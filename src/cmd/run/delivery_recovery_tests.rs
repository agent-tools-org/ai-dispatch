// Missing-delivery recovery regressions through dispatch and Codex command assembly.
// Covers feedback-bearing raw prompts, absent rollouts, and later manual retry.
use super::*;
use crate::agent::{Agent, RunOpts};
use crate::types::DeliveryAssessment;

#[tokio::test]
async fn round2_preservation_recovery_keeps_brief_feedback_and_missing_rollout() {
    let temp = TempDir::new().unwrap();
    let _home = paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = make_failed_task("t-recovery");
    task.prompt = format!(
        "[Previous attempt feedback]\nKeep citations\n\n[Original task]\n{}",
        "Complete caller brief é\n".repeat(1000)
    );
    task.resolved_prompt = Some("Old injected context".into());
    task.read_only = true;
    task.delivery_assessment = Some(DeliveryAssessment::MissingFinalDelivery);
    task.agent_session_id = Some(format!("missing-{}", uuid::Uuid::new_v4()));
    store.insert_task(&task).unwrap();
    let args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Stale arguments".into(),
        dry_run: true,
        read_only: true,
        skills: vec![NO_SKILL_SENTINEL.into()],
        ..Default::default()
    };
    let id = super::super::run_delivery_recovery::maybe_auto_recover_missing_delivery(
        &store, &task.id, &args,
    )
    .await
    .unwrap()
    .unwrap();
    let child = store.get_task(id.as_str()).unwrap().unwrap();
    assert!(child.prompt.ends_with(&task.prompt));
    assert!(
        child
            .prompt
            .contains("previous turn ended without a final deliverable")
    );
    assert!(!child.prompt.contains("Old injected context"));
    assert!(!child.prompt.contains("Stale arguments"));
    assert_eq!(child.parent_task_id.as_deref(), Some(task.id.as_str()));
    let opts = RunOpts {
        session_id: task.agent_session_id.clone(),
        dir: None, output: None, result_file: None, model: None, budget: false,
        read_only: false, sandbox: false, context_files: vec![], env: None, env_forward: None
    };
    let command = crate::agent::codex::CodexAgent
        .build_command(child.resolved_prompt.as_deref().unwrap(), &opts)
        .unwrap();
    let argv: Vec<_> = command
        .get_args()
        .map(|arg| arg.to_string_lossy())
        .collect();
    assert!(!argv.iter().any(|arg| arg == "resume"));
    assert!(argv.iter().any(|arg| arg.contains(&child.prompt)));
    assert!(
        RunArgs::for_retry(&store, &child)
            .unwrap()
            .prompt
            .ends_with(&task.prompt)
    );
}
