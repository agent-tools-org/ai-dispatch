// Tests: local validation before any send, exit-code mapping, retry policy, and API error reporting.
// Exports: none.
// Deps: super classify helpers.

use super::*;
use std::cell::Cell;

fn request(state: ClassifyState) -> ClassifyRequest {
    let questions = json!({ "ok": { "type": "noul", "instructions": "Is it ok?" } });
    ClassifyRequest {
        state,
        questions: questions.as_object().expect("map").clone(),
        model: DEFAULT_MODEL.to_string(),
        timeout_secs: DEFAULT_TIMEOUT_SECS,
        allow_secret_like: false,
    }
}

fn text(value: &str) -> ClassifyState {
    ClassifyState::Text(value.to_string())
}

#[test]
fn exit_codes_follow_the_contract() {
    let codes = [(ErrorKind::NoKey, 2), (ErrorKind::Api, 3), (ErrorKind::Invalid, 4), (ErrorKind::Refused, 5)];
    for (kind, code) in codes {
        assert_eq!(ClassifyError::new(kind, "x").exit_code(), code);
    }
}

#[test]
fn local_checks_run_before_the_key_is_read() {
    let mut no_questions = request(text("fine"));
    no_questions.questions.clear();
    assert_eq!(classify(&no_questions).unwrap_err().kind, ErrorKind::Invalid);
    assert_eq!(classify(&request(text(" "))).unwrap_err().kind, ErrorKind::Invalid);
    assert_eq!(classify(&request(text("key sk-live"))).unwrap_err().kind, ErrorKind::Refused);
    let big = "a".repeat(super::super::screen::MAX_STATE_CHARS + 1);
    assert_eq!(classify(&request(text(&big))).unwrap_err().kind, ErrorKind::Refused);
    let mut bad_model = request(text("fine"));
    bad_model.model = "jev-1\"x".into();
    assert_eq!(classify(&bad_model).unwrap_err().kind, ErrorKind::Invalid);
    let mut bad_timeout = request(text("fine"));
    bad_timeout.timeout_secs = 0;
    assert_eq!(classify(&bad_timeout).unwrap_err().kind, ErrorKind::Invalid);
}

#[test]
fn model_names_accept_versions_and_aliases_only() {
    for good in ["jev-1.13.0", "jev-latest", "jev-2"] {
        assert!(is_model_name(good), "{good}");
    }
    for bad in ["jev-", "gpt-4", "jev-1 2", "jev-a/b"] {
        assert!(!is_model_name(bad), "{bad}");
    }
}

#[test]
fn retries_once_on_rate_limit_or_overload() {
    for code in [429, 529] {
        let calls = Cell::new(0);
        let slept = Cell::new(false);
        let result = send_with_retry(
            || {
                calls.set(calls.get() + 1);
                Ok((if calls.get() == 1 { code } else { 200 }, "{}".to_string()))
            },
            |_| slept.set(true),
        );
        assert_eq!(result.expect("retried").0, 200);
        assert_eq!(calls.get(), 2);
        assert!(slept.get());
    }
}

#[test]
fn no_retry_on_other_statuses_or_after_the_second_limit() {
    for code in [401, 422, 500] {
        let calls = Cell::new(0);
        let result = send_with_retry(|| { calls.set(calls.get() + 1); Ok((code, String::new())) }, |_| {});
        assert_eq!(result.expect("status").0, code);
        assert_eq!(calls.get(), 1);
    }
    let calls = Cell::new(0);
    let result = send_with_retry(|| { calls.set(calls.get() + 1); Ok((429, String::new())) }, |_| {});
    assert_eq!((result.expect("status").0, calls.get()), (429, 2));
    let failed = send_with_retry(|| Err(anyhow::anyhow!("secret detail")), |_| {}).unwrap_err();
    assert_eq!(failed.kind, ErrorKind::Api);
    assert!(!failed.message.contains("secret detail"));
}

#[test]
fn api_errors_carry_status_and_plain_type_only() {
    let declared = BTreeMap::from([("ok".to_string(), Declared::Noul)]);
    let err = finish(422, r#"{"error":{"type":"invalid_request","message":"echo sk-abc"}}"#, &declared, Duration::ZERO)
        .unwrap_err();
    assert_eq!((err.kind, err.message.as_str()), (ErrorKind::Api, "TypeSafe API error: HTTP 422 (invalid_request)"));
    let hostile = finish(401, r#"{"type":"bad type\nwith lines"}"#, &declared, Duration::ZERO).unwrap_err();
    assert_eq!(hostile.message, "TypeSafe API error: HTTP 401");
    let invalid = finish(200, r#"{"model":"evil"}"#, &declared, Duration::ZERO).unwrap_err();
    assert_eq!(invalid.kind, ErrorKind::Api);
    assert!(!invalid.message.contains("evil"));
}

#[test]
fn success_adds_latency() {
    let declared = BTreeMap::from([("ok".to_string(), Declared::Noul)]);
    let body = r#"{"model":"jev-1.13.0","answers":{"ok":{"type":"noul","noul":0.2}},"usage":{"input_tokens":9}}"#;
    let out = finish(200, body, &declared, Duration::from_millis(42)).expect("ok");
    assert_eq!(out["latency_ms"], 42);
    assert_eq!(out["answers"]["ok"]["noul"], 0.2);
}
