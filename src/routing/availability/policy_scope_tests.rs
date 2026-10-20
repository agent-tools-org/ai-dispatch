// Regression tests for provider-scoped holds and reset formats.
// Exercises write-time classification and marker readback without live probes.

use super::*;

#[test]
fn other_agents_signatures_cannot_classify_a_hold() {
    for message in [
        "Insufficient balance",
        "You have exceeded your monthly quota",
        "Insufficient balance; try again at 2099-01-02T03:04:05Z",
    ] {
        assert_eq!(classify_hold(message, AgentKind::Codex), Hold::Transient);
        assert_eq!(
            stored_hold(&format!("message: {message}\n"), &AgentKind::Codex),
            StoredHold::Transient
        );
    }
}

#[test]
fn stray_timestamps_and_window_lengths_do_not_set_recovery() {
    for message in [
        "You have hit your usage limit; trace 2099-01-02T03:04:05Z",
        "You have hit your usage limit; 900-hour investigation",
        "You have hit your usage limit; resets in 900 hours",
        "You have hit your usage limit; try again at trace 2099-01-02T03:04:05Z",
    ] {
        let Hold::Until(at) = classify_hold(message, AgentKind::Codex) else {
            panic!("default clock")
        };
        let at = parse_recovery_datetime(&at).expect("clock");
        let minutes = (at - chrono::Local::now().naive_local()).num_minutes();
        assert!((298..=300).contains(&minutes), "{message}: {minutes}");
    }
}

#[test]
fn own_reset_phrase_accepts_only_the_adjacent_timestamp() {
    let message = "You have hit your usage limit; trace 2098-01-01T00:00:00Z; try again at 2099-01-02T03:04:05Z";
    let expected = chrono::DateTime::parse_from_rfc3339("2099-01-02T03:04:05Z")
        .expect("timestamp")
        .with_timezone(&chrono::Local)
        .naive_local();
    assert_eq!(
        classify_hold(message, AgentKind::Codex),
        Hold::Until(format_recovery(expected))
    );
    assert_eq!(classify_hold(message, AgentKind::OpenCode), Hold::Transient);
}

#[test]
fn untrusted_marker_cannot_be_promoted_when_read() {
    let marker = "source: untrusted\nmessage: Insufficient balance\n";
    assert_eq!(
        stored_hold(marker, &AgentKind::OpenCode),
        StoredHold::Transient
    );
}

#[test]
fn generic_stderr_reset_prefix_cannot_create_a_dated_hold() {
    use crate::routing::{quota::channel::Channel, rate_limit};
    let home = tempfile::tempdir().expect("home");
    let _guard = crate::support::paths::AidHomeGuard::set(home.path());
    let text = "Try again at 2027-06-01T00:00:00Z (429 Too Many Requests)";
    for agent in [
        AgentKind::Grok,
        AgentKind::Kilo,
        AgentKind::OpenCode,
        AgentKind::Oz,
        AgentKind::Custom,
    ] {
        let name = (agent == AgentKind::Custom).then_some("wrapper");
        let refusal = rate_limit::refusal_on_channel(text, agent, Channel::CliStderr)
            .expect("generic provider refusal");
        assert_eq!(classify_hold(&refusal, agent), Hold::Transient);
        rate_limit::mark_rate_limited(&agent, name, &refusal);
        let info = rate_limit::get_rate_limit_info(&agent, name).expect("transient marker");
        assert!(info.recovery_at.is_none());
        assert!(!info.needs_human);
        assert_eq!(info.message.as_deref(), Some(text));
        assert!(!rate_limit::is_rate_limited(&agent, name));
    }
}
