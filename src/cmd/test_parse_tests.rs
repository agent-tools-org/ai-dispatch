// Unit tests for libtest stdout parsing and aid test guarantees.
// Covers zero-match filters, empty targets, executed names, and failure digests.
// Deps: parent test_parse module only.

use super::*;
use std::time::Duration;

fn lines(input: &str) -> Vec<String> {
    input.lines().map(str::to_string).collect()
}

#[test]
fn parse_running_and_result_and_names() {
    let summary = parse_libtest_lines(&lines(
        r#"running 2 tests
test foo::bar ... ok
test foo::baz ... FAILED

failures:

---- foo::baz stdout ----
thread 'foo::baz' panicked at src/foo.rs:1:1:
assertion failed

failures:
    foo::baz

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.01s
"#,
    ));
    assert_eq!(summary.suites.len(), 1);
    assert_eq!(summary.suites[0].running, 2);
    assert_eq!(summary.suites[0].passed, 1);
    assert_eq!(summary.suites[0].failed, 1);
    assert!(!summary.suites[0].ok);
    assert_eq!(summary.executed.len(), 2);
    assert_eq!(summary.executed[0].name, "foo::bar");
    assert_eq!(summary.executed[1].status, TestStatus::Failed);
    assert!(summary.failure_lines.iter().any(|l| l.contains("assertion failed")));
}

#[test]
fn zero_filter_match_is_error_naming_filter() {
    let summary = parse_libtest_lines(&lines(
        r#"running 0 tests

test result: ok. 0 passed; 0 failed; 0 ignored; 0 measured; 12 filtered out; finished in 0.00s
"#,
    ));
    let verdict = evaluate_test_run(
        &summary,
        Some("no_such_test"),
        3,
        true,
        "cargo test no_such_test",
        Duration::from_millis(10),
        &[],
    );
    assert!(!verdict.success);
    assert_eq!(verdict.exit_code, 1);
    assert!(verdict.digest.contains("0 tests matched filter 'no_such_test'"));
}

#[test]
fn zero_targets_never_looks_like_pass() {
    let summary = TestHarnessSummary::default();
    let verdict = evaluate_test_run(
        &summary,
        None,
        0,
        true,
        "cargo test --lib",
        Duration::from_millis(5),
        &["error: cargo:0: no library targets found".to_string()],
    );
    assert!(!verdict.success);
    assert!(verdict.digest.contains("no test targets found"));
}

#[test]
fn pass_lists_executed_test_names() {
    let summary = parse_libtest_lines(&lines(
        r#"running 2 tests
test paths::aid_dir_uses_override ... ok
test paths::jobs_under_aid ... ok

test result: ok. 2 passed; 0 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#,
    ));
    let verdict = evaluate_test_run(
        &summary,
        Some("paths::"),
        2,
        true,
        "cargo test paths::",
        Duration::from_millis(20),
        &[],
    );
    assert!(verdict.success);
    assert!(verdict.digest.contains("ran 2 test(s):"));
    assert!(verdict.digest.contains("paths::aid_dir_uses_override"));
    assert!(verdict.digest.contains("paths::jobs_under_aid"));
}

#[test]
fn failure_digest_keeps_panic_not_pass_noise() {
    let summary = parse_libtest_lines(&lines(
        r#"running 2 tests
test ok_one ... ok
test bad_one ... FAILED

failures:

---- bad_one stdout ----
thread 'bad_one' panicked at 'boom', src/x.rs:3:5

failures:
    bad_one

test result: FAILED. 1 passed; 1 failed; 0 ignored; 0 measured; 0 filtered out; finished in 0.00s
"#,
    ));
    let verdict = evaluate_test_run(
        &summary,
        None,
        1,
        false,
        "cargo test",
        Duration::from_millis(30),
        &[],
    );
    assert!(!verdict.success);
    assert!(verdict.digest.contains("panicked at 'boom'"));
    assert!(!verdict.digest.contains("ok_one ... ok"));
}
