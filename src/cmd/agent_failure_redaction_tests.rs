// Credential-shape regressions for failure listing and review.
// Synthetic credentials only; exercises the dependency-free scanner.
use super::*;

#[test]
fn scanner_redacts_named_secrets_and_key_shaped_tokens() {
    let long_key = "SyntheticApiCredentialValue1234567890";
    let cases = [
        ("Authorization: bEaReR short:value==", "short:value=="),
        ("key = short", "short"),
        ("token: short", "short"),
        ("secret=short", "short"),
        ("password = 'short'", "short"),
        (r#"{"API_KEY": "short", "token":"other"}"#, "short"),
        ("sk-synthetic-example", "sk-synthetic-example"),
        (long_key, long_key),
        ("échec token=short\nretry", "short"),
    ];
    for (input, secret) in cases {
        let output = redact(input);
        assert!(!output.contains(secret), "{input}");
        assert!(output.contains("[REDACTED]"));
    }
    assert!(!redact(r#"{"token":"other"}"#).contains("other"));
    let diagnostic = "Provider connection closed\nretry after 30s; key rotation required: 東京";
    assert_eq!(redact(diagnostic), diagnostic);
    assert_eq!(redact(""), "");
}

#[test]
fn audit_credential_shapes_are_redacted() {
    let cases = [
        ("https://user:hunter2@host/path", "hunter2"),
        (r#""access_token": "abc""#, "abc"),
        ("refresh_token=abc", "abc"),
        ("id_token: abc", "abc"),
        ("client_secret=abc", "abc"),
        ("private_key: abc", "abc"),
        ("x-api-key=abc", "abc"),
        ("passwd=abc", "abc"),
        ("authorization=abc", "abc"),
        ("Authorization: Basic dXNlcjpwYXNz", "dXNlcjpwYXNz"),
        ("https://host/path?token=abc&key=def", "abc"),
        ("https://host/path?token=abc&key=def", "def"),
        (
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.signature",
            "eyJhbGciOiJIUzI1NiJ9",
        ),
        ("header.payload.signature", "header.payload.signature"),
        ("AKIA0000000000000000", "AKIA0000000000000000"),
        ("ASIA0000000000000000", "ASIA0000000000000000"),
        ("ghp_example", "ghp_example"),
        ("gho_example", "gho_example"),
        ("github_pat_example", "github_pat_example"),
        ("xoxa-example", "xoxa-example"),
        ("xoxb-example", "xoxb-example"),
        ("xoxp-example", "xoxp-example"),
        ("xoxr-example", "xoxr-example"),
        ("AIzaExample", "AIzaExample"),
    ];
    for (input, secret) in cases {
        assert_hidden(input, secret);
    }
    assert_eq!(redact("https://host/path"), "https://host/path");
}

fn assert_hidden(input: &str, secret: &str) {
    let store = Store::open_memory().expect("store");
    insert(&store, "milestone", input, 0, "codex");
    let groups = load(&store, None, Local::now() - chrono::Duration::days(7), 50).expect("groups");
    for output in [
        redact(input),
        serde_json::to_string(&groups).expect("JSON"),
        review_prompt(&groups).expect("review"),
    ] {
        assert!(!output.contains(secret), "{input}");
        assert!(output.contains("[REDACTED]"));
    }
}

#[test]
fn long_provider_credentials_are_redacted() {
    for prefix in ["ghp_", "xoxb-", "github_pat_", "AIza", ""] {
        let credential = format!("{prefix}{}", "SyntheticCredentialValue1234567890".repeat(2));
        assert_hidden(&credential, &credential);
    }
}
