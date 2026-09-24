// State checks before anything leaves the machine: non-empty, bounded, and not secret-like.
// Exports: MAX_STATE_CHARS, StateRefusal, screen_text, screen_json.
// Deps: serde_json.

use serde_json::Value;

pub(crate) const MAX_STATE_CHARS: usize = 100_000;
const TOKEN_PREFIXES: [&str; 7] = ["sk-", "ghp_", "github_pat_", "xox", "AKIA", "AIza", "ts_"];
const MIN_TOKEN_BODY: usize = 16;

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum StateRefusal {
    Empty,
    TooLarge(usize),
    SecretLike(&'static str),
}

impl StateRefusal {
    pub(crate) fn message(&self) -> String {
        match self {
            Self::Empty => "state is empty".into(),
            Self::TooLarge(chars) => {
                format!("state has {chars} characters; the limit is {MAX_STATE_CHARS} and it is never truncated")
            }
            Self::SecretLike(marker) => format!(
                "state looks secret-like ({marker}); pass --allow-secret-like to send it anyway"
            ),
        }
    }
}

/// Text state: non-empty, at most MAX_STATE_CHARS, and free of secret markers unless allowed.
pub(crate) fn screen_text(text: &str, allow_secret_like: bool) -> Result<(), StateRefusal> {
    if text.trim().is_empty() {
        return Err(StateRefusal::Empty);
    }
    check_size(text)?;
    match secret_marker(text) {
        Some(marker) if !allow_secret_like => Err(StateRefusal::SecretLike(marker)),
        _ => Ok(()),
    }
}

/// JSON state: sized as sent (compact), screened per string so escapes cannot hide a prefix.
pub(crate) fn screen_json(state: &Value, allow_secret_like: bool) -> Result<(), StateRefusal> {
    let empty = match state {
        Value::Object(map) => map.is_empty(),
        Value::Array(items) => items.is_empty(),
        _ => true,
    };
    if empty {
        return Err(StateRefusal::Empty);
    }
    check_size(&state.to_string())?;
    if allow_secret_like {
        return Ok(());
    }
    let mut strings = Vec::new();
    collect_strings(state, &mut strings);
    match strings.into_iter().find_map(secret_marker) {
        Some(marker) => Err(StateRefusal::SecretLike(marker)),
        None => Ok(()),
    }
}

fn check_size(text: &str) -> Result<(), StateRefusal> {
    let chars = text.chars().count();
    if chars > MAX_STATE_CHARS {
        return Err(StateRefusal::TooLarge(chars));
    }
    Ok(())
}

fn collect_strings<'a>(value: &'a Value, out: &mut Vec<&'a str>) {
    match value {
        Value::String(text) => out.push(text),
        Value::Array(items) => items.iter().for_each(|item| collect_strings(item, out)),
        Value::Object(map) => map.iter().for_each(|(key, item)| {
            out.push(key);
            collect_strings(item, out);
        }),
        _ => {}
    }
}

/// The first secret marker found: a PEM block, or a token prefix at a word start.
pub(crate) fn secret_marker(text: &str) -> Option<&'static str> {
    let pem = text.match_indices("-----BEGIN ").any(|(start, _)| {
        let line = text[start + 11..].lines().next().unwrap_or_default();
        line.contains("-----")
    });
    if pem {
        return Some("PEM block");
    }
    TOKEN_PREFIXES.into_iter().find(|prefix| {
        text.match_indices(prefix).any(|(start, _)| {
            let word_start = text[..start]
                .chars()
                .next_back()
                .is_none_or(|prev| !(prev.is_alphanumeric() || prev == '_'));
            // Real tokens carry a long body; a bare prefix in prose (`sk-`, sk-learn) is not one.
            let body = text[start + prefix.len()..]
                .chars()
                .take_while(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-'))
                .count();
            word_start && body >= MIN_TOKEN_BODY
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn token_prefixes_are_caught_at_word_starts_only() {
        let body = "A1b2C3d4E5f6G7h8";
        for prefix in ["key sk-", "sk-", "(ghp_", "\ngithub_pat_", "xoxb-", "AKIA", "=AIza", "ts_"] {
            let text = format!("{prefix}{body}");
            assert!(secret_marker(&text).is_some(), "{text:?}");
        }
        for text in ["task-list", "risk-free", "_sk-x", "bats_", "pxoxo", "cuts_it", "plain words",
                     "the `sk-` rule", "uses sk-learn", "ts_ms field", "AKIA prefix", "ghp_short"] {
            assert_eq!(secret_marker(text), None, "{text:?}");
        }
    }

    #[test]
    fn pem_blocks_are_caught() {
        let pem = "cert:\n-----BEGIN RSA PRIVATE KEY-----\nMIIE\n-----END RSA PRIVATE KEY-----";
        assert_eq!(secret_marker(pem), Some("PEM block"));
        assert_eq!(secret_marker("-----BEGIN nothing"), None);
    }

    #[test]
    fn text_state_refusals() {
        assert_eq!(screen_text("  \n", false), Err(StateRefusal::Empty));
        let big = "a".repeat(MAX_STATE_CHARS + 1);
        assert_eq!(screen_text(&big, true), Err(StateRefusal::TooLarge(MAX_STATE_CHARS + 1)));
        assert!(screen_text(&"é".repeat(MAX_STATE_CHARS), false).is_ok(), "limit counts chars, not bytes");
        assert_eq!(screen_text("token sk-123A1b2C3d4E5f6G7h8", false), Err(StateRefusal::SecretLike("sk-")));
        assert!(screen_text("token sk-123A1b2C3d4E5f6G7h8", true).is_ok());
    }

    #[test]
    fn json_state_is_screened_per_string_and_key() {
        assert_eq!(screen_json(&json!({}), false), Err(StateRefusal::Empty));
        assert_eq!(screen_json(&json!("scalar"), false), Err(StateRefusal::Empty));
        let escaped = json!({ "log": "line\nsk-hiddenA1b2C3d4E5f6G7h8" });
        assert_eq!(screen_json(&escaped, false), Err(StateRefusal::SecretLike("sk-")));
        assert!(screen_json(&json!({ "AKIA1A1b2C3d4E5f6G7h8": 1 }), false).is_err(), "keys are screened too");
        assert!(screen_json(&escaped, true).is_ok());
        assert!(screen_json(&json!([{ "ok": "clean" }]), false).is_ok());
    }

    #[test]
    fn messages_are_one_line() {
        for refusal in [StateRefusal::Empty, StateRefusal::TooLarge(5), StateRefusal::SecretLike("sk-")] {
            assert!(!refusal.message().contains('\n'));
        }
    }
}
