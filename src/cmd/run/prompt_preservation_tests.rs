// End-to-end prompt assembly regressions for task safety and omission events.
// Exercises the real bundle builder with isolated context and stored events.
use super::*;

#[test]
fn retry_task_above_token_budget_keeps_feedback_and_original_whole() {
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    let feedback = "feedback line\n".repeat(600);
    let original = "original task line\n".repeat(9000);
    let args = RunArgs {
        prompt: format!("[Previous attempt feedback]\n{feedback}\n\n[Original task]\n{original}"),
        ..Default::default()
    };
    let bundle = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        None,
        &[],
        "task-budget",
        None,
        None,
    )
    .unwrap();
    assert!(bundle.effective_prompt.contains(&args.prompt));
    assert!(bundle.effective_prompt.find(&feedback) < bundle.effective_prompt.find(&original));
}

#[test]
fn auxiliary_omission_records_one_visible_warning_event() {
    if std::env::var_os("AID_TEST_PROMPT_WARNING").is_none() {
        assert_single_warning_line();
        return;
    }
    crate::ui::output::set_quiet(true);
    let (_temp, store) = crate::support::test_env::TestHome::new().store();
    insert_prompt_task(&store, "task-warning");
    let group = store
        .create_workgroup("test", &"shared line\n".repeat(1000), None, None)
        .unwrap();
    let args = RunArgs {
        prompt: "complete task".into(),
        ..Default::default()
    };
    let bundle = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        Some(&group),
        &[],
        "task-warning",
        None,
        None,
    )
    .unwrap();
    assert!(bundle.effective_prompt.contains("complete task"));
    let warnings: Vec<_> = store
        .get_events("task-warning")
        .unwrap()
        .into_iter()
        .filter(|event| event.detail.starts_with("Prompt context bounded:"))
        .collect();
    assert_eq!(warnings.len(), 1);
    assert_eq!(warnings[0].event_kind, EventKind::Setup);
    assert!(warnings[0].detail.contains("Shared Context"));
    assert!(warnings[0].detail.contains("omitted"));
    assert_eq!(
        warnings[0].metadata.as_ref().unwrap()["type"],
        "prompt_context_bounded"
    );
}

fn assert_single_warning_line() {
    let _permit = crate::support::test_subprocess::acquire();
    let output = std::process::Command::new(std::env::current_exe().unwrap())
        .args([
            "--exact",
            "cmd::run::run_prompt::tests::preservation::auxiliary_omission_records_one_visible_warning_event",
            "--nocapture",
        ])
        .env("AID_TEST_PROMPT_WARNING", "1")
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stdout)
    );
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        stderr
            .lines()
            .filter(|line| line.starts_with("[aid] Warning: Prompt context bounded:"))
            .count(),
        1
    );
    assert!(stderr.contains("Shared Context: "));
}

fn insert_prompt_task(store: &Store, task_id: &str) {
    store
        .insert_waiting_task(
            task_id,
            "codex",
            "complete task",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
}

#[test]
fn task_over_6500_chars_is_delivered_whole_with_context_flag_without_a_warning() {
    let (temp, store) = crate::support::test_env::TestHome::new().store();
    let context = temp.path().join("context.md");
    std::fs::write(&context, "caller context").unwrap();
    let args = RunArgs {
        prompt: "task line\n".repeat(700),
        context: vec![context.to_string_lossy().into_owned()],
        ..Default::default()
    };
    let bundle = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        None,
        &[],
        "task-context",
        None,
        None,
    )
    .unwrap();
    assert!(bundle.effective_prompt.contains("[Context]"));
    assert!(
        bundle
            .effective_prompt
            .contains(&format!("[Task]\n{}", args.prompt))
    );
    assert!(store.get_events("task-context").unwrap().is_empty());
}

#[test]
fn stored_context_preservation_bounds_memory_and_rules_with_one_warning() {
    let (home, store) = crate::support::test_env::TestHome::new().store();
    insert_prompt_task(&store, "task-warning");
    store.insert_memory(&large_memory(home.path())).unwrap();
    let project = project::ProjectConfig {
        id: "test".into(),
        rules: vec!["Project rule é\n".repeat(2000)],
        ..Default::default()
    };
    let args = RunArgs {
        prompt: "complete task".into(),
        ..Default::default()
    };
    let prompt = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        None,
        &[],
        "task-warning",
        Some(&project),
        Some(home.path()),
    )
    .unwrap()
    .effective_prompt;
    assert!(prompt.contains("complete task"));
    assert!(prompt.len() < 15_000);
    let events = store.get_events("task-warning").unwrap();
    assert_eq!(events.len(), 1);
    assert!(events[0].detail.contains("Memory"));
    assert!(events[0].detail.contains("Project Rules"));
    assert!(prompt.matches("characters omitted").count() >= 2);
}

fn large_memory(root: &std::path::Path) -> Memory {
    Memory {
        id: MemoryId("m-bounded".into()),
        memory_type: MemoryType::Fact,
        tier: MemoryTier::Critical,
        content: "Memory fact é\n".repeat(2000),
        source_task_id: None,
        agent: None,
        project_path: Some(root.display().to_string()),
        content_hash: "bounded-memory".into(),
        created_at: chrono::Local::now(),
        expires_at: None,
        supersedes: None,
        version: 1,
        inject_count: 0,
        last_injected_at: None,
        success_count: 0,
    }
}
