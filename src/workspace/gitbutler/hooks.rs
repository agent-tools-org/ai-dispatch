// Hooks helpers.
// Moved from the parent module; shares its integration types and dependencies.

use super::*;

pub(super) fn read_settings_json(path: &Path) -> Result<Value> {
    if !path.exists() {
        return Ok(Value::Object(Map::new()));
    }
    let value = serde_json::from_slice::<Value>(&std::fs::read(path)?)?;
    if value.is_object() {
        Ok(value)
    } else {
        bail!("{} must contain a JSON object", path.display());
    }
}

pub(super) fn ensure_object_field<'a>(
    root: &'a mut Value,
    field: &str,
) -> Result<&'a mut Map<String, Value>> {
    let Some(root_object) = root.as_object_mut() else {
        bail!("settings root must be a JSON object");
    };
    let entry = root_object
        .entry(field.to_string())
        .or_insert_with(|| Value::Object(Map::new()));
    if !entry.is_object() {
        *entry = Value::Object(Map::new());
    }
    match entry.as_object_mut() {
        Some(object) => Ok(object),
        None => bail!("settings field '{field}' must be a JSON object"),
    }
}

pub(super) fn upsert_hook(
    hooks: &mut Map<String, Value>,
    event_name: &str,
    matcher: Option<&str>,
    command: &str,
) {
    let entry = hooks
        .entry(event_name.to_string())
        .or_insert_with(|| Value::Array(Vec::new()));
    if !entry.is_array() {
        *entry = Value::Array(Vec::new());
    }
    if let Some(items) = entry.as_array_mut() {
        let index = items
            .iter()
            .position(|value| matcher_matches(value, matcher));
        let hook_value = build_hook_value(matcher, command);
        if let Some(index) = index {
            items[index] = hook_value;
        } else {
            items.push(hook_value);
        }
    }
}

fn matcher_matches(value: &Value, matcher: Option<&str>) -> bool {
    let current = value.get("matcher").and_then(Value::as_str).unwrap_or("");
    match matcher {
        Some(expected) => current == expected,
        None => current.is_empty(),
    }
}

fn build_hook_value(matcher: Option<&str>, command: &str) -> Value {
    match matcher {
        Some(matcher) => json!({
            "matcher": matcher,
            "hooks": [{"type": "command", "command": command}],
        }),
        None => json!({
            "hooks": [{"type": "command", "command": command}],
        }),
    }
}
