// Test module wrapper for `cmd::run_prompt`.
// Exports: nested test modules for helper, skill, and sanitize coverage.
// Deps: `run_prompt/tests.rs`, `run_prompt/skill_tests.rs`, run_prompt internals.

use super::*;

#[path = "run_prompt/tests.rs"]
mod extracted_tests;

#[path = "run_prompt/skill_tests.rs"]
mod skill_tests;

#[path = "run_prompt/worktree_paths_tests.rs"]
mod worktree_paths_tests;

#[path = "run_prompt/rust_cache_prompt_tests.rs"]
mod rust_cache_prompt_tests;

#[test]
fn sanitize_strips_structural_tags() {
    let input = "keep\n<aid-project-rules>\ninside\n</aid-team-rules>\nend";
    let sanitized = sanitize_injected_text(input);
    assert_eq!(sanitized, "keep\nend");
}

#[test]
fn sanitize_preserves_normal_lines() {
    let input = "alpha\n beta\n[Task]\nplain text";
    let sanitized = sanitize_injected_text(input);
    assert_eq!(sanitized, input);
}

#[test]
fn audit_report_bundle_omits_implementation_instructions() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let skills_dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(
        skills_dir.join("implementer.md"),
        "Verify: compile, run existing tests, add tests, commit",
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let args = RunArgs {
        prompt: format!(
            "Read-only audit of branch X. Report findings and do not modify files. {}",
            "Inspect all relevant behavior. ".repeat(8),
        ),
        result_file: Some("result-task.md".to_string()),
        ..Default::default()
    };

    let bundle = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        None,
        &["implementer".to_string()],
        "task-audit",
    )
    .unwrap();

    assert!(!bundle.effective_prompt.contains("--- Methodology ---"));
    assert!(!bundle.effective_prompt.contains("Git Staging Rule"));
    assert!(bundle.effective_prompt.contains("## Findings"));
    assert!(bundle.effective_prompt.contains("<aid-result-file>result-task.md</aid-result-file>"));
}

#[test]
fn write_task_mentioning_read_only_audit_keeps_implementation_instructions() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let skills_dir = crate::paths::aid_dir().join("skills");
    std::fs::create_dir_all(&skills_dir).unwrap();
    std::fs::write(skills_dir.join("implementer.md"), "implementation method").unwrap();
    let store = Store::open_memory().unwrap();
    let args = RunArgs {
        prompt: format!(
            "Add unit tests for the read-only audit module. {}",
            "Validate the behavior thoroughly. ".repeat(8),
        ),
        ..Default::default()
    };

    let bundle = build_prompt_bundle(
        &store,
        &args,
        &AgentKind::Codex,
        None,
        &["implementer".to_string()],
        "task-write",
    )
    .unwrap();

    assert!(bundle.effective_prompt.contains("Git Staging Rule"));
    assert!(bundle.effective_prompt.contains("implementation method"));
}
