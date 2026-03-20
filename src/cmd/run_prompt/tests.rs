// Tests for `cmd::run_prompt` helpers and verify-failure retry behavior.
// Exports: none.
// Deps: run_prompt helpers, in-memory Store, temporary PATH/AID_HOME setup.

use super::*;
use crate::test_subprocess;
use std::ffi::{OsStr, OsString};
use std::sync::Arc;

struct EnvVarGuard {
    key: &'static str,
    previous: Option<OsString>,
}

impl EnvVarGuard {
    fn set(key: &'static str, value: impl AsRef<OsStr>) -> Self {
        let previous = std::env::var_os(key);
        unsafe { std::env::set_var(key, value) };
        Self { key, previous }
    }
}

impl Drop for EnvVarGuard {
    fn drop(&mut self) {
        match &self.previous {
            Some(value) => unsafe { std::env::set_var(self.key, value) },
            None => unsafe { std::env::remove_var(self.key) },
        }
    }
}

fn run_args(skills: Vec<String>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "prompt".to_string(),
        skills,
        ..Default::default()
    }
}

fn build_prompt_args(output: Option<&str>) -> RunArgs {
    RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Write the requested content".to_string(),
        output: output.map(str::to_string),
        ..Default::default()
    }
}

#[test]
fn format_batch_siblings_truncates_and_limits_output() {
    let prompt = "a".repeat(81);
    let siblings = (0..12)
        .map(|idx| {
            (
                format!("task-{idx}"),
                "codex".to_string(),
                prompt.clone(),
            )
        })
        .collect::<Vec<_>>();

    let formatted = format_batch_siblings(&siblings);

    assert!(formatted.contains("- \"task-0\" (codex):"));
    assert!(formatted.contains(&format!("{}...", "a".repeat(80))));
    assert!(!formatted.contains("\"task-10\""));
    assert!(formatted.contains("+ 2 more"));
}

#[test]
fn effective_skills_auto_apply_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();
    assert_eq!(
        effective_skills(&AgentKind::Codex, &run_args(vec![])),
        vec!["implementer"]
    );
}

#[test]
fn effective_skills_respect_no_skill_sentinel() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();
    assert!(
        effective_skills(
            &AgentKind::Codex,
            &run_args(vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()])
        )
        .is_empty()
    );
}

#[test]
fn extract_words_normalizes_keywords() {
    let text = "Refactor Foo::Bar and update src/lib.rs to fix Config::load().";
    let words = super::prompt_context::extract_words(text);
    assert!(words.contains("refactor"));
    assert!(words.contains("foo"));
    assert!(words.contains("bar"));
    assert!(!words.contains("src")); // "src" is a stop word
    assert!(words.contains("lib"));
    assert!(words.contains("rs"));
    assert!(words.contains("config"));
    assert!(words.contains("load"));
}

#[test]
fn build_prompt_bundle_includes_output_instruction_when_output_is_set() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(Some("out.txt")),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("Your final response will be saved to a file."));
}

#[test]
fn build_prompt_bundle_omits_output_instruction_when_output_is_not_set() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Your final response will be saved to a file."));
}

#[test]
fn build_prompt_bundle_includes_shared_dir_instruction_when_env_is_set() {
    let shared_dir = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("AID_SHARED_DIR", shared_dir.path());
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("[Shared Directory]"));
    assert!(bundle.effective_prompt.contains(&shared_dir.path().display().to_string()));
}

#[test]
fn fill_empty_output_from_log_populates_zero_byte_file() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        log.path(),
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"human-readable output\"}\n",
    )
    .unwrap();
    std::fs::write(output.path(), "").unwrap();

    fill_empty_output_from_log(log.path(), Some(output.path())).unwrap();

    assert_eq!(
        std::fs::read_to_string(output.path()).unwrap(),
        "human-readable output"
    );
}

#[test]
fn fill_empty_output_from_log_keeps_existing_output() {
    let log = tempfile::NamedTempFile::new().unwrap();
    let output = tempfile::NamedTempFile::new().unwrap();
    std::fs::write(
        log.path(),
        "{\"type\":\"message\",\"role\":\"assistant\",\"content\":\"replacement\"}\n",
    )
    .unwrap();
    std::fs::write(output.path(), "existing").unwrap();

    fill_empty_output_from_log(log.path(), Some(output.path())).unwrap();

    assert_eq!(std::fs::read_to_string(output.path()).unwrap(), "existing");
}

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

    fill_empty_output_from_log(log.path(), Some(output.path())).unwrap();

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

    let work_dir = temp.path().join("work");
    std::fs::create_dir_all(&work_dir).unwrap();
    let store = Arc::new(Store::open_memory().unwrap());
    let root_id = crate::cmd::run::run(
        store.clone(),
        RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Fix the build".to_string(),
            dir: Some(work_dir.to_string_lossy().to_string()),
            verify: Some("false".to_string()),
            retry: 1,
            skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()],
            ..Default::default()
        },
    )
    .await
    .unwrap();

    let retried = store.get_task(root_id.as_str()).unwrap().unwrap();
    let all_tasks = store.list_tasks(TaskFilter::All).unwrap();
    let original = all_tasks
        .iter()
        .find(|task| task.parent_task_id.is_none())
        .unwrap();

    assert_eq!(all_tasks.len(), 2);
    // Original stays Done because auto-retry triggered (enforce skipped via early return)
    // Retried stays Done with VFAIL — verify failed but work is preserved
    assert_eq!(original.status, TaskStatus::Done);
    assert_eq!(original.verify_status, VerifyStatus::Failed);
    assert_eq!(retried.parent_task_id.as_deref(), Some(original.id.as_str()));
    assert_eq!(retried.verify.as_deref(), Some("false"));
    assert_eq!(retried.status, TaskStatus::Done);
    assert_eq!(retried.verify_status, VerifyStatus::Failed);
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

#[test]
fn build_prompt_bundle_skips_sibling_context_for_opencode() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-opencode-test"))
        .unwrap();
    
    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        read_only: false,
        budget: false,
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::OpenCode,
        None,
        &[],
        "task-1",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Sibling Task Context"));
}

#[test]
fn build_prompt_bundle_skips_sibling_context_for_kilo() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-kilo-test"))
        .unwrap();
    
    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling-kilo".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        read_only: false,
        budget: false,
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "kilo".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::Kilo,
        None,
        &[],
        "task-kilo",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Sibling Task Context"));
}

#[test]
fn build_prompt_bundle_includes_sibling_context_for_codex() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    crate::paths::ensure_dirs().unwrap();
    let store = Store::open_memory().unwrap();
    let group = store
        .create_workgroup("batch", "desc", Some("seed"), Some("wg-codex-test"))
        .unwrap();
    
    let sibling_task: crate::types::Task = crate::types::Task {
        id: crate::types::TaskId("task-sibling-codex".to_string()),
        agent: crate::types::AgentKind::Codex,
        custom_agent_name: None,
        prompt: "Sibling task prompt".to_string(),
        resolved_prompt: None,
        status: TaskStatus::Done,
        parent_task_id: None,
        workgroup_id: Some(group.id.to_string()),
        caller_kind: None,
        caller_session_id: None,
        agent_session_id: None,
        repo_path: None,
        worktree_path: None,
        worktree_branch: None,
        log_path: None,
        output_path: None,
        tokens: Some(100),
        prompt_tokens: None,
        duration_ms: Some(1000),
        model: None,
        cost_usd: None,
        exit_code: None,
        created_at: chrono::Local::now(),
        completed_at: None,
        verify: None,
        verify_status: crate::types::VerifyStatus::Skipped,
        read_only: false,
        budget: false,
    };
    store.insert_task(&sibling_task).unwrap();
    let summary = crate::cmd::summary::CompletionSummary {
        task_id: sibling_task.id.as_str().to_string(),
        agent: "codex".to_string(),
        status: "done".to_string(),
        files_changed: vec!["src/lib.rs".to_string()],
        summary_text: "Task completed".to_string(),
        conclusion: String::new(),
        duration_secs: Some(1),
        token_count: Some(100),
    };
    store.save_completion_summary(sibling_task.id.as_str(), &serde_json::to_string(&summary).unwrap()).unwrap();

    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            agent_name: "codex".to_string(),
            prompt: "Write the requested content".to_string(),
            group: Some(group.id.to_string()),
            ..Default::default()
        },
        &AgentKind::Codex,
        None,
        &[],
        "task-codex",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("Sibling Task Context"));
}
