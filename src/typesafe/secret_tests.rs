// Tests: the TypeSafe key never reaches argv, the curl config is well formed, lookups are bounded.
// Exports: none.
// Deps: super secret helpers.

use super::*;

const KEY: &str = "ts_test_0123456789abcdef";

#[test]
fn argv_never_contains_key_or_body() {
    let args = curl_args("https://api.typesafe.ai/v1/systemone", 10);
    assert!(args.iter().all(|arg| !arg.contains(KEY) && !arg.contains("Bearer")));
    assert!(args.windows(2).any(|pair| pair[0] == "-K" && pair[1] == "-"));
}

#[test]
fn config_carries_key_header_and_escaped_body() {
    let body = r#"{"state":"say \"hi\"\nnow","q":"a\\b"}"#;
    let config = curl_config(KEY, body).expect("config");
    assert!(config.starts_with(&format!("header = \"Authorization: Bearer {KEY}\"\n")));
    let data_line = config.lines().nth(1).expect("data line");
    assert_eq!(data_line, r#"data-binary = "{\"state\":\"say \\\"hi\\\"\\nnow\",\"q\":\"a\\\\b\"}""#);
    assert_eq!(config.lines().count(), 2, "a body cannot inject extra config lines");
}

#[test]
fn keys_that_could_inject_config_are_refused() {
    for bad in ["", "a\"b", "a\\b", "a\nurl = \"http://evil\"", "a\rb"] {
        assert!(curl_config(bad, "{}").is_err(), "{bad:?}");
    }
}

#[test]
fn status_suffix_is_split_from_body() {
    assert_eq!(parse_status_suffix("{\"ok\":1}\n200").unwrap(), (200, "{\"ok\":1}".to_string()));
    assert_eq!(parse_status_suffix("line1\nline2\n422").unwrap().0, 422);
    assert!(parse_status_suffix("\n000").is_err(), "curl reports 000 when no response arrived");
    assert!(parse_status_suffix("no status").is_err());
}

#[test]
fn stalled_lookup_is_killed_at_the_deadline() {
    let mut sleeper = Command::new("sleep");
    sleeper.arg("30").stdout(Stdio::piped());
    let started = Instant::now();
    assert!(output_within(sleeper, Duration::from_millis(200)).is_none());
    assert!(started.elapsed() < Duration::from_secs(5), "killed promptly, not after 30 s");
}

#[test]
fn finished_lookup_returns_stdout_and_failure_returns_none() {
    let mut echo = Command::new("printf");
    echo.arg("value").stdout(Stdio::piped());
    assert_eq!(output_within(echo, Duration::from_secs(5)).as_deref(), Some(&b"value"[..]));
    let mut failing = Command::new("false");
    failing.stdout(Stdio::piped());
    assert!(output_within(failing, Duration::from_secs(5)).is_none());
}
