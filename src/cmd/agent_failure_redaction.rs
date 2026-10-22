// Credential redaction for untrusted provider diagnostics.
// Preserves diagnostic separators and strips URL userinfo before token scanning.
pub(super) fn redact(raw: &str) -> String {
    let cleaned = strip_userinfo(raw);
    let raw = cleaned.as_str();
    let mut output = String::with_capacity(raw.len());
    let mut offset = 0;
    let mut redact_next = false;
    while let Some(first) = raw[offset..].chars().next() {
        if token_separator(first) {
            output.push(first);
            offset += first.len_utf8();
            continue;
        }
        let rest = &raw[offset..];
        let end = rest
            .find(|ch: char| token_separator(ch) && (!redact_next || !matches!(ch, '=' | ':')))
            .unwrap_or(rest.len());
        let token = &rest[..end];
        let lower = token.to_ascii_lowercase();
        if redact_next || key_shaped(token) {
            output.push_str("[REDACTED]");
            redact_next = matches!(lower.as_str(), "basic" | "bearer");
        } else {
            output.push_str(token);
            let following = rest[end..]
                .trim_start_matches(|ch: char| ch.is_whitespace() || matches!(ch, '\'' | '"'));
            let named_secret = [
                "token",
                "secret",
                "key",
                "password",
                "passwd",
                "authorization",
            ]
            .iter()
            .any(|suffix| lower.ends_with(*suffix));
            redact_next = matches!(lower.as_str(), "bearer" | "basic")
                || (named_secret && following.starts_with(['=', ':']));
        }
        offset += end;
    }
    output
}

fn token_separator(ch: char) -> bool {
    ch.is_whitespace()
        || matches!(
            ch,
            '=' | ':' | ',' | ';' | '\'' | '"' | '{' | '}' | '[' | ']' | '(' | ')' | '?' | '&'
        )
}

fn key_shaped(token: &str) -> bool {
    let lower = token.to_ascii_lowercase();
    let known_prefix = [
        "sk-",
        "akia",
        "asia",
        "ghp_",
        "gho_",
        "github_pat_",
        "xoxa-",
        "xoxb-",
        "xoxp-",
        "xoxr-",
        "aiza",
    ]
    .iter()
    .any(|prefix| lower.starts_with(*prefix));
    let jwt = token.bytes().filter(|ch| *ch == b'.').count() == 2
        && token
            .bytes()
            .all(|ch| ch.is_ascii_alphanumeric() || b"_-.".contains(&ch));
    known_prefix
        || jwt
        || (token.len() >= 32
            && token
                .bytes()
                .all(|ch| ch.is_ascii_alphanumeric() || b"_-+/".contains(&ch)))
}

fn strip_userinfo(raw: &str) -> String {
    let mut output = String::new();
    let mut remaining = raw;
    while let Some(start) = remaining.find("://") {
        let host_start = start + 3;
        output.push_str(&remaining[..host_start]);
        remaining = &remaining[host_start..];
        let end = remaining
            .find(|ch: char| ch.is_whitespace() || matches!(ch, '/' | '?' | '#'))
            .unwrap_or(remaining.len());
        if let Some(at) = remaining[..end].rfind('@') {
            output.push_str("[REDACTED]@");
            remaining = &remaining[at + 1..];
        }
    }
    output.push_str(remaining);
    output
}
