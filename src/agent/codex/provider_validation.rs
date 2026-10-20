// Provider value validation and inline credential rejection.
// Shares the same allowlist with command construction and read-only provenance.

pub(super) fn known_setting(key: &str) -> bool {
    matches!(
        key,
        "name"
            | "wire_api"
            | "env_key_instructions"
            | "base_url"
            | "env_key"
            | "query_params"
            | "env_http_headers"
            | "request_max_retries"
            | "stream_max_retries"
            | "stream_idle_timeout_ms"
            | "requires_openai_auth"
            | "supports_websockets"
    )
}

pub(crate) fn safe_setting(key: &str, value: &toml::Value) -> bool {
    if inline_credentials(key, value) {
        return false;
    }
    match key {
        "name" | "wire_api" | "env_key_instructions" => value.is_str(),
        "base_url" => value.as_str().is_some_and(|url| {
            let authority = url.split_once("://").map(|(_, rest)| rest).unwrap_or(url);
            !authority.contains(['@', '?', '#'])
        }),
        "env_key" => value.as_str().is_some_and(safe_env),
        "query_params" => value.as_table().is_some_and(|params| {
            params.iter().all(|(key, value)| {
                !credential_key(key) && value.is_str() && !literal_credential(value)
            })
        }),
        "env_http_headers" => value.as_table().is_some_and(|headers| {
            headers
                .values()
                .all(|value| value.as_str().is_some_and(safe_env))
        }),
        "request_max_retries" | "stream_max_retries" | "stream_idle_timeout_ms" => {
            value.as_integer().is_some_and(|number| number >= 0)
        }
        "requires_openai_auth" | "supports_websockets" => value.is_bool(),
        _ => false,
    }
}

pub(super) fn inline_credentials(key: &str, value: &toml::Value) -> bool {
    if matches!(key, "env_key" | "env_http_headers") {
        return false;
    }
    matches!(key, "experimental_bearer_token" | "http_headers")
        || credential_key(key)
        || literal_credential(value)
        || (key == "query_params"
            && value
                .as_table()
                .is_some_and(|params| params.keys().any(|key| credential_key(key))))
}

pub(super) fn credential_key(key: &str) -> bool {
    let key = key.to_ascii_lowercase().replace('-', "_");
    key == "key"
        || [
            "api_key",
            "access_key",
            "private_key",
            "client_key",
            "credential",
            "signature",
            "access_token",
            "token",
            "secret",
            "password",
            "authorization",
        ]
        .iter()
        .any(|part| key.contains(part))
            && !key.starts_with("env_")
}

pub(super) fn literal_credential(value: &toml::Value) -> bool {
    match value {
        toml::Value::String(text) => {
            let text = text.to_ascii_lowercase();
            text.contains("bearer ")
                || text.contains("-----begin")
                || text.contains("api_key=")
                || text.contains("token=")
                || text
                    .split(|c: char| c.is_whitespace() || ['/', ':', '=', '\"', '\''].contains(&c))
                    .any(|word| {
                        ["sk-", "sk_", "ghp_", "xoxb-"]
                            .iter()
                            .any(|prefix| word.starts_with(prefix))
                    })
        }
        toml::Value::Table(table) => table.values().any(literal_credential),
        toml::Value::Array(values) => values.iter().any(literal_credential),
        _ => false,
    }
}

fn safe_env(name: &str) -> bool {
    !name.is_empty()
        && name.bytes().all(|c| c.is_ascii_alphanumeric() || c == b'_')
        && ![
            "CODEX_HOME",
            "CODEX_SQLITE_HOME",
            "HOME",
            "PATH",
            "AID_HOME",
            "CARGO_HOME",
            "RUSTUP_HOME",
            "TMPDIR",
        ]
        .contains(&name)
}
