// Fail-closed diagnostic allowlist for failure-marker display.
// Keeps classification words, codes and hostnames; secret triggers mask the line tail.
pub(super) fn redact(raw: &str) -> String {
    raw.split_inclusive('\n').map(line).collect()
}

fn line(raw: &str) -> String {
    let mut output = String::new();
    let mut offset = 0;
    let body = raw.trim_end_matches(['\r', '\n']);
    for part in body.split_inclusive(char::is_whitespace) {
        let token = part.trim_end_matches(char::is_whitespace);
        let core = token.trim_matches(|ch: char| ch.is_ascii_punctuation());
        let name = token
            .split(['=', ':'])
            .next()
            .unwrap_or("")
            .trim_matches(|ch: char| ch.is_ascii_punctuation());
        let lower = name.to_ascii_lowercase();
        let trigger = concat!(
            "authorization cookie session sessionid sid password passwd passphrase pwd ",
            "secret token key credential credentials signature sig auth bearer basic"
        )
        .split_whitespace()
        .find(|suffix| lower.ends_with(*suffix));
        if let Some(trigger) = trigger
            && (matches!(lower.as_str(), "bearer" | "basic")
                || token.contains(['=', ':'])
                || !raw[offset + token.len()..].trim().is_empty())
        {
            output.push_str(&name[name.len() - trigger.len()..]);
            output.push_str(" [REDACTED]");
            break;
        }
        output.push_str(&if core.contains("://") {
            url(token.trim_start_matches(|ch: char| ch.is_ascii_punctuation()))
        } else if core.is_empty() || allowed(core) {
            token.to_string()
        } else {
            "[…]".into()
        });
        output.push_str(&part[token.len()..]);
        offset += part.len();
    }
    output.push_str(&raw[body.len()..]);
    output
}

fn word(token: &str) -> bool {
    let bytes = token.as_bytes();
    if bytes.is_empty()
        || bytes.len() > 20
        || !bytes[0].is_ascii_alphabetic()
        || !bytes[bytes.len() - 1].is_ascii_alphabetic()
        || bytes
            .iter()
            .any(|b| !b.is_ascii_alphabetic() && !b"-_'".contains(b))
    {
        return false;
    }
    let lower = token.to_ascii_lowercase();
    token[1..] == lower[1..] || (token.len() <= 12 && token == token.to_ascii_uppercase())
}

// Region and model labels need a few trailing digits; longer runs can carry IDs or card numbers.
const MAX_LABEL_DIGITS: usize = 8;

fn host(token: &str) -> bool {
    token.split('.').all(|label| {
        let base = label.trim_end_matches(|ch: char| ch.is_ascii_digit());
        label.len() <= 20
            && base == base.to_ascii_lowercase()
            && (word(base)
                || ((1..=MAX_LABEL_DIGITS).contains(&(label.len() - base.len()))
                    && word(base.trim_end_matches('-'))))
    })
}

fn allowed(token: &str) -> bool {
    let version = token.strip_prefix('v').unwrap_or(token);
    word(token)
        || (token.len() <= 6 && token.bytes().all(|ch| ch.is_ascii_digit()))
        || (token.len() <= 12
            && version.contains('.')
            && version
                .split('.')
                .all(|part| !part.is_empty() && part.bytes().all(|ch| ch.is_ascii_digit())))
        || ((token.contains('.') || token.contains('-')) && host(token))
}

fn url(token: &str) -> String {
    let Some((scheme, rest)) = token.split_once("://") else {
        return "[…]".into();
    };
    if !word(scheme) || scheme != scheme.to_ascii_lowercase() {
        return "[…]".into();
    }
    let authority = rest.split(['/', '?', '#', '"', '\'']).next().unwrap_or("");
    let hostport = authority.rsplit('@').next().unwrap_or("");
    let hostname = hostport.split(':').next().unwrap_or("");
    let userinfo = if authority.contains('@') {
        "[…]@"
    } else {
        ""
    };
    let hostname = if host(hostname) { hostname } else { "[…]" };
    let tail = if rest != authority { "[…]" } else { "" };
    format!("{scheme}://{userinfo}{hostname}{tail}")
}
