// Unit tests for cargo target-dir permission detection and fallback selection.
// Covers EPERM/EACCES path matching, unrelated errors, and same-path skip.
// Deps: parent build_fallback module only.

use super::*;

#[test]
fn detects_sandbox_eperm_on_exact_target_path() {
    let stderr = vec![
        "error: Operation not permitted (os error 1) at path \"/Users/me/.cargo-target/ai-dispatch/feat-x\""
            .to_string(),
    ];
    assert!(target_dir_permission_blocked(
        &stderr,
        "/Users/me/.cargo-target/ai-dispatch/feat-x"
    ));
}

#[test]
fn detects_permission_denied_when_cargo_appends_lock_suffix() {
    // cargo may create `{target}{random}` beside an unwritable target dir
    let stderr = vec![
        "error: Permission denied (os error 13) at path \"/tmp/ro/blockedBUcX5R\"".to_string(),
    ];
    assert!(target_dir_permission_blocked(&stderr, "/tmp/ro/blocked"));
}

#[test]
fn ignores_permission_errors_for_unrelated_paths() {
    let stderr = vec![
        "error: Operation not permitted (os error 1) at path \"/var/forbidden\"".to_string(),
    ];
    assert!(!target_dir_permission_blocked(
        &stderr,
        "/Users/me/.cargo-target/ai-dispatch/feat-x"
    ));
}

#[test]
fn ignores_non_permission_cargo_failures() {
    let stderr = vec!["error: could not compile `ai-dispatch`".to_string()];
    assert!(!target_dir_permission_blocked(&stderr, "/tmp/target"));
}

#[test]
fn retry_skipped_when_cargo_succeeded() {
    assert!(should_retry_with_fallback(true, &[], Some("/tmp/shared")).is_none());
}

#[test]
fn retry_skipped_when_no_target_dir() {
    let stderr = vec![
        "error: Operation not permitted (os error 1) at path \"/tmp/x\"".to_string(),
    ];
    assert!(should_retry_with_fallback(false, &stderr, None).is_none());
}

#[test]
fn digest_note_names_both_paths() {
    let note = fallback_digest_note("/shared/a", "/work/target");
    assert!(note.contains("/shared/a"));
    assert!(note.contains("/work/target"));
    assert!(note.contains("fell back"));
}
