// Tests that the TypeSafe key never reaches argv and that the curl config is well formed.
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
