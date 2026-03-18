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
            &run_args(vec![super::super::NO_SKILL_SENTINEL.to_string()])
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
    let root_id = super::super::run(
        store.clone(),
        RunArgs {
            agent_name: "opencode".to_string(),
            prompt: "Fix the build".to_string(),
            dir: Some(work_dir.to_string_lossy().to_string()),
            verify: Some("false".to_string()),
            retry: 1,
            skills: vec![super::super::NO_SKILL_SENTINEL.to_string()],
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
    assert_eq!(original.status, TaskStatus::Done);
    assert_eq!(original.verify_status, VerifyStatus::Failed);
    assert_eq!(retried.parent_task_id.as_deref(), Some(original.id.as_str()));
    assert_eq!(retried.verify.as_deref(), Some("false"));
    assert_eq!(retried.status, TaskStatus::Done);
    assert_eq!(retried.verify_status, VerifyStatus::Failed);
    assert!(retried.prompt.contains(VERIFY_RETRY_FEEDBACK));
}
