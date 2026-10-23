// Replace one top-level payload string without reserializing preserved JSON values.
// Input is validated by retention; duplicate payload keys are conservatively skipped.
pub(super) fn replace_string(raw: &str, key: &str, marker: &str) -> Option<String> {
    let bytes = raw.as_bytes();
    let mut depth = 0;
    let mut index = 0;
    let mut found = None;
    while index < bytes.len() {
        match bytes[index] {
            b'{' | b'[' => depth += 1,
            b'}' | b']' => depth -= 1,
            b'"' => {
                let end = string_end(bytes, index)?;
                let colon = skip_space(bytes, end);
                if depth == 1
                    && bytes.get(colon) == Some(&b':')
                    && serde_json::from_str::<String>(&raw[index..end])
                        .ok()?
                        .as_str()
                        == key
                {
                    if found.is_some() {
                        return None;
                    }
                    let start = skip_space(bytes, colon + 1);
                    if bytes.get(start) != Some(&b'"') {
                        return None;
                    }
                    found = Some((start, string_end(bytes, start)?));
                }
                index = end;
                continue;
            }
            _ => {}
        }
        index += 1;
    }
    let (start, end) = found?;
    Some(format!("{}{marker}{}", &raw[..start], &raw[end..]))
}

fn string_end(bytes: &[u8], start: usize) -> Option<usize> {
    let mut index = start + 1;
    while index < bytes.len() {
        match bytes[index] {
            b'\\' => index += 2,
            b'"' => return Some(index + 1),
            _ => index += 1,
        }
    }
    None
}

fn skip_space(bytes: &[u8], mut index: usize) -> usize {
    while bytes.get(index).is_some_and(u8::is_ascii_whitespace) {
        index += 1;
    }
    index
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn preserves_all_other_values_byte_for_byte() {
        let raw = r#"{ "tokens": 1e3, "full" : "tool\noutput", "route": { "x": "a\\\"b" } }"#;
        let updated = replace_string(raw, "full", r#"{"trimmed":true,"bytes":11}"#).unwrap();
        assert_eq!(
            updated,
            r#"{ "tokens": 1e3, "full" : {"trimmed":true,"bytes":11}, "route": { "x": "a\\\"b" } }"#
        );
    }

    #[test]
    fn respects_nested_keys_escaped_keys_and_duplicate_keys() {
        assert_eq!(
            replace_string(r#"{"a":{"full":"nested"},"\u0066ull":"out"}"#, "full", "{}"),
            Some(r#"{"a":{"full":"nested"},"\u0066ull":{}}"#.into())
        );
        assert!(replace_string(r#"{"full":"one","full":"two"}"#, "full", "{}").is_none());
    }

    #[test]
    fn provider_replacement_preserves_full_markers_and_protected_json_bytes() {
        let raw =
            r#"{ "full":{"trimmed":true,"bytes":4096}, "provider_stdout":"out", "cost":2.50 }"#;
        let updated = replace_string(raw, "provider_stdout", "{}").unwrap();
        assert_eq!(
            updated,
            r#"{ "full":{"trimmed":true,"bytes":4096}, "provider_stdout":{}, "cost":2.50 }"#
        );
        assert!(
            replace_string(
                r#"{"provider_stdout":"a","provider_stdout":"b"}"#,
                "provider_stdout",
                "{}"
            )
            .is_none()
        );
    }
}
