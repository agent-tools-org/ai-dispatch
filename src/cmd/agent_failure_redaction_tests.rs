// Credential-shape regressions for failure listing in text and JSON.
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
        ("sk-synthetic-example123", "sk-synthetic-example123"),
        (long_key, long_key),
        ("échec token=short\nretry", "short"),
    ];
    for (input, secret) in cases {
        let output = redact(input);
        assert!(!output.contains(secret), "{input}");
        assert!(output.contains("[REDACTED]") || output.contains("[…]"));
    }
    assert!(!redact(r#"{"token":"other"}"#).contains("other"));
    let diagnostic = "Provider connection closed\nretry after 30s";
    assert_eq!(
        redact(diagnostic),
        "Provider connection closed\nretry after […]"
    );
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
            "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxIn0.signature123",
            "eyJhbGciOiJIUzI1NiJ9",
        ),
        ("AKIA0000000000000000", "AKIA0000000000000000"),
        ("ASIA0000000000000000", "ASIA0000000000000000"),
        ("ghp_AbCdEfGhIj1234567890", "ghp_AbCdEfGhIj1234567890"),
        ("gho_AbCdEfGhIj1234567890", "gho_AbCdEfGhIj1234567890"),
        (
            "github_pat_AbCdEfGhIj1234567890",
            "github_pat_AbCdEfGhIj1234567890",
        ),
        (
            concat!("xoxa-", "AbCdEfGhIj1234567890"),
            concat!("xoxa-", "AbCdEfGhIj1234567890"),
        ),
        (
            concat!("xoxb-", "AbCdEfGhIj1234567890"),
            concat!("xoxb-", "AbCdEfGhIj1234567890"),
        ),
        (
            concat!("xoxp-", "AbCdEfGhIj1234567890"),
            concat!("xoxp-", "AbCdEfGhIj1234567890"),
        ),
        (
            concat!("xoxr-", "AbCdEfGhIj1234567890"),
            concat!("xoxr-", "AbCdEfGhIj1234567890"),
        ),
        ("AIzaExample", "AIzaExample"),
    ];
    for (input, secret) in cases {
        assert_hidden(input, secret);
    }
    assert_eq!(redact("https://host/path"), "https://host[…]");
}

fn assert_hidden(input: &str, secret: &str) {
    let store = Store::open_memory().expect("store");
    insert(&store, "milestone", input, 0, "codex");
    let groups = load(&store, None, Local::now() - chrono::Duration::days(7), 50).expect("groups");
    for output in [
        redact(input),
        serde_json::to_string(&groups).expect("JSON"),
        listing(&groups),
    ] {
        assert!(!output.contains(secret), "{input}");
        assert!(output.contains("[REDACTED]") || output.contains("[…]"));
    }
}

#[test]
fn long_provider_credentials_are_redacted() {
    for prefix in ["ghp_", "xoxb-", "github_pat_", "AIza", ""] {
        let credential = format!("{prefix}{}", "SyntheticCredentialValue1234567890".repeat(2));
        assert_hidden(&credential, &credential);
    }
}

#[test]
fn diagnostic_domains_versions_and_regions_are_preserved() {
    for input in [
        "api.openai.com",
        "codex 0.46.0",
        "v1.2.3",
        "foo.tar.gz",
        "asia-northeast1",
        "Asia",
        "akia-short",
        "aiza-short",
    ] {
        assert_eq!(redact(input), input);
    }
}

#[test]
fn classification_diagnostics_survive_the_allowlist() {
    for diagnostic in [
        "429 Too Many Requests",
        "rate limit exceeded for model gpt-5",
        "api.openai.com",
        "codex 0.46.0",
        "asia-northeast1",
        "HTTP 401 Unauthorized",
        "overloaded_error",
        "Missing required field messages",
        "generativelanguage.googleapis.com",
        "can't connect",
        "123456",
        "--- : = [] {}",
    ] {
        assert_eq!(redact(diagnostic), diagnostic);
    }
}

#[test]
fn secret_triggers_mask_the_entire_line_tail() {
    for trigger in concat!(
        "authorization cookie session sessionid sid password passwd passphrase pwd ",
        "secret token key credential credentials signature sig auth bearer basic"
    )
    .split_whitespace()
    {
        for separator in ["=", ": ", " ", "\": \""] {
            let input = format!(
                "error {trigger}{separator}correct horse, battery; staple\nHTTP 401 Unauthorized"
            );
            assert_eq!(
                redact(&input),
                format!("error {trigger} [REDACTED]\nHTTP 401 Unauthorized")
            );
        }
    }
    assert_eq!(redact("Bearer"), "Bearer [REDACTED]");
    assert_eq!(redact("opaque-AbCdEfGhIj.sig next"), "sig [REDACTED]");
    assert_eq!(redact("basic\r\nretry"), "basic [REDACTED]\r\nretry");
}

const THIRD_AUDIT_CASES: &[(&str, &str)] = &[
    (
        r#"password: "correct horse battery staple""#,
        "horse battery staple",
    ),
    (r#""client_secret": "ab,cd;ef""#, "cd;ef"),
    ("--token abc123", "abc123"),
    ("--password hunter2", "hunter2"),
    ("Authorization: Token abc123def", "abc123def"),
    ("Authorization: ApiKey abc123def", "abc123def"),
    ("Authorization: Key abc123def", "abc123def"),
    ("Authorization: SSWS abc123def", "abc123def"),
    ("Authorization: Digest response=abc123def", "abc123def"),
    ("Proxy-Authorization: Token abc123def", "abc123def"),
    (
        "Cookie: session=s3cr3tSessionId; csrftoken=x",
        "s3cr3tSessionId",
    ),
    (
        "Set-Cookie: sessionid=synthetic-session",
        "synthetic-session",
    ),
    ("sid=abc123", "abc123"),
    ("connect.sid=s%3Asigned.cookie", "signed.cookie"),
    (
        concat!("glpat-", "AbCdEfGhIj1234567890"),
        concat!("glpat-", "AbCdEfGhIj1234567890"),
    ),
    (
        "glpat-AbCdEfGhIj.dotted.1234567890",
        concat!("glpat-", "AbCdEfGhIj"),
    ),
    (
        concat!("xoxs-", "AbCdEfGh123456"),
        concat!("xoxs-", "AbCdEfGh123456"),
    ),
    (
        concat!("xoxc-", "AbCdEfGh123456"),
        concat!("xoxc-", "AbCdEfGh123456"),
    ),
    ("xoxe.xoxp-AbCdEfGh123456", "xoxe.xoxp-AbCdEfGh123456"),
    ("xapp-AbCdEfGh123456", "xapp-AbCdEfGh123456"),
    ("ghs_AbCdEfGh123456", "ghs_AbCdEfGh123456"),
    ("ghu_AbCdEfGh123456", "ghu_AbCdEfGh123456"),
    ("ghr_AbCdEfGh123456", "ghr_AbCdEfGh123456"),
    ("ts_AbCdEfGh123456", "ts_AbCdEfGh123456"),
    ("?sig=abc%2Bdef%3D", "abc%2Bdef%3D"),
    ("?code=abc%2Bdef", "abc%2Bdef"),
    ("?auth=abc123", "abc123"),
    ("?signature=abc%2Bdef", "abc%2Bdef"),
    (
        "ya29.a0AfSynthetic1234567890",
        "ya29.a0AfSynthetic1234567890",
    ),
    (
        "eyJhbGciOiJub25lIn0.eyJzdWIiOiIxIn0.",
        "eyJhbGciOiJub25lIn0",
    ),
    (
        "eyJhbGciOiJSU0EifQ.encrypted.iv.cipher.tag",
        "eyJhbGciOiJSU0EifQ",
    ),
    ("HTTP_PROXY=user:pass@proxy:8080", "pass@proxy"),
    ("https://user:pass/word@host/path", "pass/word"),
    ("https://user:pass?word@host/path", "pass?word"),
    ("https://user:pass#word@host/path", "pass#word"),
];

#[test]
fn third_audit_leaking_shapes_are_masked_in_listing_outputs() {
    for (input, secret) in THIRD_AUDIT_CASES {
        assert_hidden(input, secret);
    }
}

#[test]
fn unrecognised_values_are_masked_and_urls_keep_only_safe_hosts() {
    for opaque in [
        "AbCdEfGhIj",
        "1234567",
        "échec",
        "eyJshort.payload.signature",
        "abc123",
        "verylonglowercasewordbeyondlimit",
        "UPPERCASETOOLONG",
        "label1middle.example.com",
    ] {
        assert_eq!(redact(opaque), "[…]", "{opaque}");
    }
    assert_eq!(redact("https://api.openai.com"), "https://api.openai.com");
    assert_eq!(
        redact("https://api.openai.com/"),
        "https://api.openai.com[…]"
    );
    assert_eq!(
        redact("https://user:pass@api.openai.com/v1?code=opaque"),
        "https://[…]@api.openai.com[…]"
    );
    assert_eq!(redact("https://AbCdEfGhIj/path"), "https://[…][…]");
    assert_eq!(redact("{}\n\nretry\n"), "{}\n\nretry\n");
}
