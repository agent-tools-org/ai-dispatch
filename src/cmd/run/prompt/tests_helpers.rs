// Extracted helpers tests from tests.rs.
// Deps: parent test fixtures and module imports.
use super::*;

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

/// Omitting `--skill` means no skill, not a skill chosen from the agent kind.
/// `auto_skills` used to hand `implementer` to every implementation CLI and
/// `researcher` to gemini and agy regardless of the task, spending the caller's
/// tokens on a persona nobody asked for.
#[test]
fn effective_skills_default_to_none_when_nothing_is_declared() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();
    // The project default is passed in rather than detected, so this asserts
    // what it claims — that nothing is invented from the agent kind — instead
    // of asserting that the developer's own project declares no skill.
    assert!(effective_skills(&run_args(vec![]), None).is_empty());
}

/// A project default is used only when the caller declared nothing, and an
/// explicit "no skills" still beats it.
#[test]
fn project_default_skill_applies_only_when_nothing_is_declared() {
    let project = crate::project::ProjectConfig { skills: vec!["implementer".to_string()], ..Default::default() };
    assert_eq!(
        effective_skills(&run_args(vec![]), Some(&project)),
        vec!["implementer"]
    );
    assert_eq!(
        effective_skills(&run_args(vec!["reviewer".to_string()]), Some(&project)),
        vec!["reviewer"]
    );
    assert!(
        effective_skills(
            &run_args(vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()]),
            Some(&project)
        )
        .is_empty()
    );
}

#[test]
fn a_declared_skill_is_used_verbatim() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    assert_eq!(
        effective_skills(&run_args(vec!["reviewer".to_string()]), None),
        vec!["reviewer"]
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
        effective_skills(&run_args(vec![crate::cmd::run::NO_SKILL_SENTINEL.to_string()]), None)
            .is_empty()
    );
}

#[test]
fn resolve_worktree_paths_rejects_read_only_worktrees() {
    let err = resolve_worktree_paths(
        &RunArgs {
            worktree: Some("wt-readonly".to_string()),
            read_only: true,
            ..Default::default()
        },
        None,
    )
    .unwrap_err();

    assert!(err
        .to_string()
        .contains("--read-only cannot be used with --worktree"));
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
        &build_prompt_args(Some("out.txt"), None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("Your final response will be saved to a file."));
}

#[test]
fn build_prompt_bundle_includes_result_file_instruction_when_result_file_is_set() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None, Some("/tmp/result.md")),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("<aid-result-file>/tmp/result.md</aid-result-file>"));
    assert!(bundle.effective_prompt.contains("This file will be preserved as the task's official result."));
}

#[test]
fn build_prompt_bundle_omits_output_instruction_when_output_is_not_set() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None, None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("Your final response will be saved to a file."));
}

#[test]
fn build_prompt_bundle_includes_git_staging_guard_for_writable_tasks() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None, None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains(crate::templates::git_staging_guard().trim()));
}

#[test]
fn build_prompt_bundle_omits_git_staging_guard_for_read_only_tasks() {
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &RunArgs {
            read_only: true,
            ..build_prompt_args(None, None)
        },
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("git add <newfile>"));
}

#[test]
fn build_prompt_bundle_includes_shared_dir_instruction_when_env_is_set() {
    let shared_dir = tempfile::tempdir().unwrap();
    let _guard = EnvVarGuard::set("AID_SHARED_DIR", shared_dir.path());
    let store = Store::open_memory().unwrap();
    let bundle = build_prompt_bundle(
        &store,
        &build_prompt_args(None, None),
        &AgentKind::Codex,
        None,
        &[],
        "task-1",
        None,
        None,
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

    fill_empty_output_from_log(log.path(), Some(output.path()), None).unwrap();

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

    fill_empty_output_from_log(log.path(), Some(output.path()), None).unwrap();

    assert_eq!(std::fs::read_to_string(output.path()).unwrap(), "existing");
}
