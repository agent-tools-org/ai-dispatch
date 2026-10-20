// Qwen terminal refusal attribution and bare-token anti-forgery coverage.
// Deps: parent channel-test helpers and Qwen result envelope shape.
use super::*;

#[test]
fn the_terminal_result_slot_is_open_for_qwen_and_shut_for_everyone_else() {
    let envelope = serde_json::json!({"type":"result", "is_error":false,
        "result":"Quota exhausted: Your token-plan 5-hour quota has been exhausted."})
    .to_string();
    assert!(stream(&envelope, AgentKind::Qwen).contains("Quota exhausted:"));
    assert!(stream(&envelope, AgentKind::Cursor).is_empty());
    assert!(stream_diagnostic(&envelope, AgentKind::Qwen).is_empty());
}

#[test]
fn qwens_result_slot_is_never_strong_enough_for_a_bare_status_token() {
    let envelope = serde_json::json!({"type":"result", "is_error":false,
        "result":"The upstream RPC returned 429 twice during the run."})
    .to_string();
    assert!(stream(&envelope, AgentKind::Qwen).is_empty());
    assert!(
        stream_diagnostic(&envelope, AgentKind::Qwen).is_empty(),
        "the model's own slot is never CLI-diagnostic evidence"
    );
}
