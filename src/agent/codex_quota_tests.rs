// Codex quota write-path: live wording still marks via refusal_on_channel.
// Loaded by `codex.rs` under `#[cfg(test)]`.

use super::CodexAgent;
use crate::agent::Agent;
use crate::types::{AgentKind, EventKind, TaskId};
use crate::{paths, rate_limit};

#[test]
fn live_usage_limit_error_envelope_marks() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = paths::AidHomeGuard::set(temp.path());
    rate_limit::clear_rate_limit(&AgentKind::Codex, None);
    let line = r#"{"type":"error","message":"You have hit your usage limit. try again at Mar 21st, 2099 2:27 PM."}"#;
    let event = CodexAgent
        .parse_event(&TaskId("t-codex".to_string()), line)
        .unwrap();
    assert_eq!(event.event_kind, EventKind::Error);
    assert!(rate_limit::is_rate_limited(&AgentKind::Codex, None));
    rate_limit::clear_rate_limit(&AgentKind::Codex, None);
}
