// Regression coverage for auxiliary bounds and raw worker continuation prompts.
// Exercises prompt assembly, saved worker args, and omission event aggregation.
use super::*;

fn insert_task(store: &Store, id: &str, output: Option<&str>, group: Option<&str>) {
    store
        .insert_waiting_task(
            id,
            "codex",
            "Caller brief",
            None,
            group,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    if let Some(output) = output {
        store.update_output_path(id, output).unwrap();
    }
}

fn bundle(store: &Store, args: &RunArgs, skills: &[String]) -> PromptBundle {
    build_prompt_bundle(
        store,
        args,
        &AgentKind::Codex,
        None,
        skills,
        "t-current",
        None,
        None,
    )
    .unwrap()
}

fn assert_bounded(store: &Store, prompt: &str, label: &str) {
    assert!(prompt.contains("Caller brief"));
    assert!(
        prompt.len() < 15_000,
        "unbounded {label}: {} bytes",
        prompt.len()
    );
    assert!(
        prompt.contains("characters omitted"),
        "no in-place omission for {label}"
    );
    let warnings: Vec<_> = store
        .get_events("t-current")
        .unwrap()
        .into_iter()
        .filter(|event| event.detail.starts_with("Prompt context bounded:"))
        .collect();
    assert_eq!(warnings.len(), 1);
    assert!(warnings[0].detail.contains(label));
}

#[test]
fn sibling_file_list_preservation_bounds_auxiliary_and_warns_once() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let group = store.create_workgroup("siblings", "", None, None).unwrap();
    insert_task(&store, "t-current", None, Some(group.id.as_str()));
    insert_task(&store, "t-sibling", None, Some(group.id.as_str()));
    store
        .update_task_status("t-sibling", TaskStatus::Skipped)
        .unwrap();
    let summary = CompletionSummary {
        task_id: "t-sibling".into(),
        agent: "codex".into(),
        status: "skipped".into(),
        files_changed: (0..4000)
            .map(|n| format!("src/very_long_file_name_{n}.rs"))
            .collect(),
        summary_text: String::new(),
        conclusion: "Conclusion preserved".into(),
        duration_secs: None,
        token_count: None,
    };
    store
        .save_completion_summary("t-sibling", &serde_json::to_string(&summary).unwrap())
        .unwrap();
    let args = RunArgs {
        prompt: "Caller brief".into(),
        group: Some(group.id.to_string()),
        ..Default::default()
    };
    assert_bounded(
        &store,
        &bundle(&store, &args, &[]).effective_prompt,
        "Sibling Task Context",
    );
}

#[test]
fn prior_output_preservation_bounds_auxiliary_and_warns_once() {
    let home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let output = home.path().join("output.md");
    std::fs::write(&output, "Prior result é\n".repeat(8000)).unwrap();
    insert_task(&store, "t-prior", Some(output.to_str().unwrap()), None);
    insert_task(&store, "t-current", None, None);
    let args = RunArgs {
        prompt: "Caller brief".into(),
        context_from: vec!["t-prior".into()],
        ..Default::default()
    };
    assert_bounded(
        &store,
        &bundle(&store, &args, &[]).effective_prompt,
        "Prior Task Result",
    );
}

#[test]
fn shared_output_preservation_bounds_auxiliary_and_warns_once() {
    if std::env::var_os("AID_TEST_SHARED_BOUND").is_none() {
        let _permit = crate::support::test_subprocess::acquire();
        let home = tempfile::tempdir().unwrap();
        std::fs::write(
            home.path().join("result.md"),
            "Shared result é\n".repeat(8000),
        )
        .unwrap();
        let output = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                concat!(
                    "cmd::run::run_prompt::tests::auxiliary_preservation::",
                    "shared_output_preservation_bounds_auxiliary_and_warns_once"
                ),
                "--nocapture",
            ])
            .env("AID_TEST_SHARED_BOUND", "1")
            .env("AID_SHARED_DIR", home.path())
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stdout)
        );
        return;
    }
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    insert_task(&store, "t-current", None, None);
    let args = RunArgs {
        prompt: "Caller brief".into(),
        context_from: vec!["shared:result.md".into()],
        ..Default::default()
    };
    assert_bounded(
        &store,
        &bundle(&store, &args, &[]).effective_prompt,
        "Shared File",
    );
}

#[test]
fn methodology_preservation_bounds_auxiliary_and_aggregates_warning() {
    let home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    insert_task(&store, "t-current", None, None);
    let skills = crate::support::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&skills).unwrap();
    std::fs::write(skills.join("large.md"), "Methodology é\n".repeat(8000)).unwrap();
    let output = home.path().join("prior.md");
    std::fs::write(&output, "Prior result\n".repeat(8000)).unwrap();
    insert_task(&store, "t-prior", Some(output.to_str().unwrap()), None);
    let args = RunArgs {
        prompt: "Caller brief".into(),
        skills: vec!["large".into()],
        context_from: vec!["t-prior".into()],
        ..Default::default()
    };
    assert_bounded(
        &store,
        &bundle(&store, &args, &args.skills).effective_prompt,
        "Methodology",
    );
    let warnings = store.get_events("t-current").unwrap();
    assert!(warnings[0].detail.contains("Prior Task Result"));
}

#[test]
fn background_cascade_preservation_does_not_accumulate_launch_context() {
    let _home = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("cascade", "Injected shared context", None, None)
        .unwrap();
    let mut args = RunArgs {
        prompt: "Caller brief".into(),
        ..Default::default()
    };
    for n in 0..3 {
        let id = format!("t-cascade-{n}");
        insert_task(&store, &id, None, None);
        store
            .update_task_dispatch_args(&id, &args.dispatch_args_json().unwrap())
            .unwrap();
        let launch = build_prompt_bundle(
            &store,
            &args,
            &AgentKind::Codex,
            Some(&group),
            &[],
            &id,
            None,
            None,
        )
        .unwrap();
        store
            .update_resolved_prompt(&id, &launch.effective_prompt)
            .unwrap();
        let spec = serde_json::from_value(serde_json::json!({
            "task_id": id, "worker_pid": null, "agent_name": "codex", "prompt": "ignored",
            "dir": null, "output": null, "model": null, "verify": null, "retry": 0,
            "group": null, "interactive": false,
        }))
        .unwrap();
        args = RunArgs::for_worker(&store, &spec).unwrap().clone();
        assert_eq!(args.prompt, "Caller brief");
        assert_eq!(
            launch
                .effective_prompt
                .matches("Injected shared context")
                .count(),
            1
        );
    }
}

#[test]
fn template_preservation_appends_brief_without_placeholder_on_dispatch_and_retry() {
    let _home = crate::support::test_env::TestHome::new();
    let templates = crate::support::paths::aid_dir().join("templates");
    std::fs::create_dir_all(&templates).unwrap();
    std::fs::write(templates.join("broken.md"), "Unrelated task").unwrap();
    let store = Store::open_memory().unwrap();
    for prompt in [
        "Caller brief",
        "[Previous attempt feedback]\nFix it\n\nCaller brief",
    ] {
        let args = RunArgs {
            prompt: prompt.into(),
            template: Some("broken".into()),
            ..Default::default()
        };
        let bundle = build_prompt_bundle(
            &store,
            &args,
            &AgentKind::Codex,
            None,
            &[],
            "t-template",
            None,
            None,
        )
        .unwrap();
        assert!(bundle.effective_prompt.contains("Unrelated task"));
        assert!(bundle.effective_prompt.contains(prompt), "template dropped the caller brief");
    }
}
