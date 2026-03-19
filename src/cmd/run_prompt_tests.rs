// Test module wrapper for `cmd::run_prompt`.
// Exports: nested test modules for helper, skill, and sanitize coverage.
// Deps: `run_prompt/tests.rs`, `run_prompt/skill_tests.rs`, run_prompt internals.

use super::*;

#[path = "run_prompt/tests.rs"]
mod extracted_tests;

#[path = "run_prompt/skill_tests.rs"]
mod skill_tests;

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
