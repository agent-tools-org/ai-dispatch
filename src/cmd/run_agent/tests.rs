// Tests for streaming output persistence in `aid run`.
// Covers substantive message retention, filtering, and mixed streaming completion events.
// Depends on super::write_streaming_output, serde_json, and tempfile.

use super::write_streaming_output;
use serde_json::json;
use tempfile::NamedTempFile;

#[test]
fn write_streaming_output_keeps_last_five_substantive_messages() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let content = [
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "short ack" }
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message one is long enough to survive the substantive filter threshold." }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "Message two starts in a streamed chunk and remains comfortably above ",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "the substantive cutoff once the second streamed chunk arrives.",
            "delta": true
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message three is another detailed update that should be retained." }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "Message four is a buffered assistant message with enough detail to keep."
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message five records a later milestone with enough context to count." }
        }),
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "Message six is the newest substantive message and should push the oldest one out." }
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(
        output,
        [
            "Message two starts in a streamed chunk and remains comfortably above the substantive cutoff once the second streamed chunk arrives.",
            "Message three is another detailed update that should be retained.",
            "Message four is a buffered assistant message with enough detail to keep.",
            "Message five records a later milestone with enough context to count.",
            "Message six is the newest substantive message and should push the oldest one out.",
        ]
        .join("\n\n---\n\n")
    );
}

#[test]
fn write_streaming_output_skips_writing_when_messages_are_not_substantive() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let content = [
        json!({
            "type": "item.completed",
            "item": { "type": "agent_message", "text": "short ack" }
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "tiny delta",
            "delta": true
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();
    std::fs::write(out_file.path(), "existing output").unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(output, "existing output");
}

#[test]
fn write_streaming_output_does_not_duplicate_streamed_message_when_final_message_matches() {
    let log_file = NamedTempFile::new().unwrap();
    let out_file = NamedTempFile::new().unwrap();
    let message = "This streamed message is long enough to remain substantive after assembly.";
    let content = [
        json!({
            "type": "message",
            "role": "assistant",
            "content": "This streamed message is long enough ",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": "to remain substantive after assembly.",
            "delta": true
        }),
        json!({
            "type": "message",
            "role": "assistant",
            "content": message
        }),
    ]
    .iter()
    .map(serde_json::to_string)
    .collect::<Result<Vec<_>, _>>()
    .unwrap()
    .join("\n");
    std::fs::write(log_file.path(), content).unwrap();

    write_streaming_output(log_file.path(), out_file.path());

    let output = std::fs::read_to_string(out_file.path()).unwrap();
    assert_eq!(output, message);
}
