// Extracted instructions tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

#[test]
fn fill_empty_output_from_log_falls_back_to_raw_text() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        log.path(),
        "plain output line 1\n{\"type\":\"completion\",\"tokens\":1}\nplain output line 2\n",
    )
    .unwrap();
    std::fs::write(output.path(), "").unwrap();

    fill_empty_output_from_log(log.path(), Some(output.path()), None).unwrap();

    assert_eq!(
        std::fs::read_to_string(output.path()).unwrap(),
        "plain output line 1\nplain output line 2"
    );
}

#[test]
fn clean_output_if_jsonl_cleans_jsonl_file() {
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        output.path(),
        concat!(
            "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"first message\"}\n",
            "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"second message\"}\n"
        ),
    )
    .unwrap();

    clean_output_if_jsonl(output.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(output.path()).unwrap(),
        "first message\n---\nsecond message"
    );
}

#[test]
fn clean_output_if_jsonl_preserves_normal_text() {
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(output.path(), "normal output\nsecond line\n").unwrap();

    clean_output_if_jsonl(output.path()).unwrap();

    assert_eq!(
        std::fs::read_to_string(output.path()).unwrap(),
        "normal output\nsecond line\n"
    );
}

#[test]
fn clean_output_if_jsonl_preserves_mixed_content() {
    let output = tempfile::NamedTempFile::new().unwrap();
    let mixed = concat!(
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"json message\"}\n",
        "plain line one\n",
        "plain line two\n"
    );
    std::fs::write(output.path(), mixed).unwrap();

    clean_output_if_jsonl(output.path()).unwrap();

    assert_eq!(std::fs::read_to_string(output.path()).unwrap(), mixed);
}

#[test]
fn extract_raw_text_from_log_ignores_aid_sentinel_lines() {
    let file = tempfile::NamedTempFile::new().unwrap();
    let lines = concat!(
        "{\"type\":\"unknown\"}\n",
        "Warning: Reached maximum conversation turns (100).\n",
        "=== AID TASK t-test FAILED (exit 8) ===\n"
    );
    std::fs::write(file.path(), lines).unwrap();

    let raw = extract_output_fallback_from_path(file.path(), None);
    assert_ne!(
        raw.as_deref(),
        Some("=== AID TASK t-test FAILED (exit 8) ==="),
        "Sentinel line must not be returned as output"
    );
}

#[test]
fn build_prompt_bundle_appends_batch_siblings_after_system_context() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-batch"))
        .unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            batch_siblings: vec![(
                "task-2".to_string(),
                "gemini".to_string(),
                "Summarize the dependency graph".to_string(),
            )],
            ..Default::default()
        },
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    let system_idx = bundle.effective_prompt.find("<aid-system-context>").unwrap();
    let siblings_idx = bundle.effective_prompt.find("<aid-batch-siblings>").unwrap();

    assert!(siblings_idx > system_idx);
    assert!(bundle.effective_prompt.contains("- \"task-2\" (gemini): Summarize the dependency graph"));
}

#[tokio::test]
async fn run_auto_retries_after_verify_failure() {
    let _permit = test_subprocess::acquire();
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();

    let bin_dir = temp.path().join("bin");
    std::fs::create_dir_all(&bin_dir).unwrap();
    let script_path = bin_dir.join("opencode");
    std::fs::write(
        &script_path,
        "#!/bin/sh\nprintf '%s\\n' '{\"type\":\"completion\",\"tokens\":1}'\n",
    )
    .unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mut permissions = std::fs::metadata(&script_path).unwrap().permissions();
        permissions.set_mode(0o755);
        std::fs::set_permissions(&script_path, permissions).unwrap();
    }

    let path_value = OsString::from(format!("{}:/bin:/usr/bin", bin_dir.display()));
    let _path = EnvVarGuard::set("PATH", &path_value);
    let _task_id_guard = EnvVarGuard::remove("AID_TASK_ID");

    let verify_counter_path = temp.path().join("verify-count");
    let verify_path = temp.path().join("verify-on-retry.sh");
    std::fs::write(
        &verify_path,
        "#!/bin/sh\ncount=0\nif [ -f \"$1\" ]; then count=$(cat \"$1\"); fi\ncount=$((count + 1))\nprintf '%s\\n' \"$count\" > \"$1\"\nif [ \"$count\" -eq 1 ]; then exit 1; fi\nexit 0\n",
    )
    .unwrap();

    let work_dir = temp.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let final_id = crate::cmd::run::run(
        store.clone(),
        RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Fix the build".to_string(),
            dir: Some(work_dir.to_string_lossy().to_string()),
            verify: Some(format!(
                "sh {} {}",
                verify_path.display(),
                verify_counter_path.display()
            )),
            retry: 1,
            skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retried = store.get_task(final_id.as_str()).unwrap().unwrap();
    let all_tasks = store.list_tasks(TaskFilter::All).unwrap();
    let original = all_tasks
        .iter()
        .find(|task| Some(task.id.as_str()) == retried.parent_task_id.as_deref())
        .unwrap();
    let run_status = crate::cmd_dispatch::DispatchOutcome::Run(
        crate::cmd_dispatch::RunDispatch::new(final_id, false, false),
    )
    .run_exit_status(store.as_ref())
    .unwrap()
    .unwrap();

    assert_eq!(all_tasks.len(), 2);
    assert_eq!(original.status, TaskStatus::Failed);
    assert_eq!(original.verify_status, VerifyStatus::Failed);
    assert_eq!(original.exit_code, Some(1));
    assert_eq!(retried.parent_task_id.as_deref(), Some(original.id.as_str()));
    assert_eq!(retried.status, TaskStatus::Done);
    assert_eq!(retried.verify_status, VerifyStatus::Passed);
    assert_eq!(retried.exit_code, Some(0));
    assert_eq!(run_status.exit_code(), 0);
    assert!(retried.prompt.contains(VERIFY_RETRY_FEEDBACK));
}

#[test]
fn load_workgroup_returns_none_when_group_id_is_none() {
    let store = Store::open_memory().unwrap();
    let result = load_workgroup(&store, None).unwrap();
    assert!(result.is_none());
}

#[test]
fn load_workgroup_returns_existing_workgroup() {
    let store = Store::open_memory().unwrap();
    let created = store.create_workgroup("test-group", "", Some("test"), Some("wg-test")).unwrap();
    let loaded = load_workgroup(&store, Some("wg-test")).unwrap().unwrap();
    assert_eq!(loaded.id, created.id);
    assert_eq!(loaded.name, "test-group");
}

#[test]
fn load_workgroup_auto_creates_when_not_found() {
    let store = Store::open_memory().unwrap();
    let loaded = load_workgroup(&store, Some("wg-new")).unwrap().unwrap();
    assert_eq!(loaded.id.as_str(), "wg-new");
    assert_eq!(loaded.name, "wg-new");
    assert_eq!(loaded.created_by.as_deref(), Some("auto"));
    let found = store.get_workgroup("wg-new").unwrap().unwrap();
    assert_eq!(found.id, loaded.id);
}
