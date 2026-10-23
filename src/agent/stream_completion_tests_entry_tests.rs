// Production entry point regression tests split to keep the parent within its size limit.
// Deps: parent test fixtures and production APIs.

use super::*;

/// The live false positive of 2026-08-07, end to end.
///
/// A cursor audit task read `src/agent/cursor_tests.rs:142` and quoted it into
/// its report. aid wrote `~/.aid/rate-limit-cursor`, held cursor until the next
/// day and recorded the task FAILED with exit code 0, while cursor served the
/// whole time. The report reaches the watcher as assistant envelopes, which is
/// where the split now drops it.
#[test]
fn a_report_quoting_this_repos_own_fixture_never_marks_the_agent() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::support::paths::AidHomeGuard::set(temp.path());
    let cursor = crate::types::AgentKind::Cursor;
    crate::routing::rate_limit::clear_all_rate_limits_for_agent(&cursor, None);

    let quoted = "assert_rate_limit(r#\"{\"type\":\"error\",\"message\":\"quota exceeded for \
                  this workspace\"}\"#, true);\n====\ncommit 7881e2d";
    let output = format!(
        "{}\n{}\n",
        serde_json::json!({"type": "system", "subtype": "init", "model": "composer-2.5"}),
        serde_json::json!({
            "type": "assistant",
            "message": {"content": [{"type": "text", "text": quoted}]}
        })
    );

    assert!(!record_quota_exhaustion_with_delivery(&output, cursor, None, None, false).recorded());
    assert!(!crate::routing::rate_limit::is_rate_limited(&cursor, None));
    assert!(!crate::routing::rate_limit::is_group_rate_limited(&cursor, None, "premium"));
}
