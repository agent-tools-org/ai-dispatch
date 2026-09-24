// Typed TypeSafe questions: flag pairing, merge with a native questions map, and validation.
// Exports: FlagKind, FlagEvent, questions_from_flags, merge_questions, validate_questions, Declared.
// Deps: serde_json.

use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

const MAX_ID_CHARS: usize = 64;

/// One question-building flag, in argv order, so `--choice` pairs with the next `--options`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum FlagKind {
    Noul,
    Choice,
    Options,
    Score,
    Levels,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct FlagEvent {
    pub kind: FlagKind,
    pub value: String,
}

/// What a question declared, kept to validate its answer.
#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum Declared {
    Noul,
    Choice(Vec<String>),
    Score(usize),
}

/// Native question entries built from flags, in argv order.
pub(crate) fn questions_from_flags(events: &[FlagEvent]) -> Result<Vec<(String, Value)>, String> {
    let mut built = Vec::new();
    let mut pending: Option<(FlagKind, String, String)> = None;
    for event in events {
        match event.kind {
            FlagKind::Noul => {
                let (id, question) = split_id(&event.value, "--noul")?;
                built.push((id, json!({ "type": "noul", "instructions": question })));
            }
            FlagKind::Choice | FlagKind::Score => {
                if let Some((kind, id, _)) = &pending {
                    return Err(format!("{} {id} has no {}", flag_name(*kind), partner_name(*kind)));
                }
                let (id, question) = split_id(&event.value, flag_name(event.kind))?;
                pending = Some((event.kind, id, question));
            }
            FlagKind::Options | FlagKind::Levels => {
                let head = if event.kind == FlagKind::Options { FlagKind::Choice } else { FlagKind::Score };
                match pending.take() {
                    Some((kind, id, question)) if kind == head => {
                        built.push((id, paired_entry(kind, &question, &event.value)?));
                    }
                    _ => return Err(format!("{} must follow a {}", flag_name(event.kind), flag_name(head))),
                }
            }
        }
    }
    if let Some((kind, id, _)) = pending {
        return Err(format!("{} {id} has no {}", flag_name(kind), partner_name(kind)));
    }
    Ok(built)
}

fn paired_entry(kind: FlagKind, question: &str, list: &str) -> Result<Value, String> {
    let items = split_list(list, partner_name(kind))?;
    if kind == FlagKind::Choice {
        let criteria: Map<String, Value> = items.into_iter().map(|option| (option, Value::Null)).collect();
        return Ok(json!({ "type": "choice", "instructions": question, "criteria": criteria }));
    }
    Ok(json!({ "type": "score", "instructions": question, "criteria": items }))
}

fn split_id(value: &str, flag: &str) -> Result<(String, String), String> {
    let (id, question) = value
        .split_once('=')
        .ok_or_else(|| format!("{flag} expects ID=QUESTION"))?;
    if question.trim().is_empty() {
        return Err(format!("{flag} {id} has an empty question"));
    }
    Ok((id.to_string(), question.to_string()))
}

fn split_list(list: &str, flag: &str) -> Result<Vec<String>, String> {
    let items: Vec<String> = list.split(',').map(|item| item.trim().to_string()).collect();
    if items.iter().any(String::is_empty) {
        return Err(format!("{flag} has an empty entry"));
    }
    for (index, item) in items.iter().enumerate() {
        if items[..index].contains(item) {
            return Err(format!("{flag} repeats '{item}'"));
        }
    }
    Ok(items)
}

fn flag_name(kind: FlagKind) -> &'static str {
    match kind {
        FlagKind::Noul => "--noul",
        FlagKind::Choice => "--choice",
        FlagKind::Options => "--options",
        FlagKind::Score => "--score",
        FlagKind::Levels => "--levels",
    }
}

fn partner_name(kind: FlagKind) -> &'static str {
    if kind == FlagKind::Choice { "--options" } else { "--levels" }
}

/// The native map from `--questions` plus flag-built entries; an id used twice is refused.
pub(crate) fn merge_questions(
    file: Option<Map<String, Value>>,
    flags: Vec<(String, Value)>,
) -> Result<Map<String, Value>, String> {
    let mut merged = file.unwrap_or_default();
    for (id, entry) in flags {
        if merged.contains_key(&id) {
            return Err(format!("duplicate question id '{id}'"));
        }
        merged.insert(id, entry);
    }
    Ok(merged)
}

/// Checks every entry against the TypeSafe question shapes and returns what each declared.
pub(crate) fn validate_questions(questions: &Map<String, Value>) -> Result<BTreeMap<String, Declared>, String> {
    if questions.is_empty() {
        return Err("at least one question is required".into());
    }
    questions
        .iter()
        .map(|(id, entry)| {
            validate_id(id)?;
            validate_entry(entry).map(|declared| (id.clone(), declared)).map_err(|err| format!("question '{id}': {err}"))
        })
        .collect()
}

fn validate_id(id: &str) -> Result<(), String> {
    let charset_ok = id.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '_' | '-' | '.'));
    if id.is_empty() || id.chars().count() > MAX_ID_CHARS || !charset_ok {
        return Err("question ids must be 1-64 characters of letters, digits, '_', '-', '.'".into());
    }
    Ok(())
}

fn validate_entry(entry: &Value) -> Result<Declared, String> {
    let object = entry.as_object().ok_or("must be an object")?;
    if let Some(key) = object.keys().find(|key| !matches!(key.as_str(), "type" | "instructions" | "criteria")) {
        return Err(format!("unknown field '{key}'"));
    }
    match object.get("instructions") {
        Some(Value::String(text)) if !text.trim().is_empty() => {}
        Some(Value::Object(_) | Value::Array(_)) => {}
        _ => return Err("instructions must be a non-empty string, object, or array".into()),
    }
    let criteria = object.get("criteria");
    match object.get("type").and_then(Value::as_str) {
        Some("noul") => validate_noul(criteria),
        Some("choice") => validate_choice(criteria),
        Some("score") => validate_score(criteria),
        _ => Err("type must be noul, choice, or score".into()),
    }
}

fn validate_noul(criteria: Option<&Value>) -> Result<Declared, String> {
    match criteria {
        None => Ok(Declared::Noul),
        Some(Value::Object(map)) if map.keys().all(|key| key == "true" || key == "false") => Ok(Declared::Noul),
        Some(_) => Err("noul criteria may only hold 'true' and 'false'".into()),
    }
}

fn validate_choice(criteria: Option<&Value>) -> Result<Declared, String> {
    let map = criteria.and_then(Value::as_object).ok_or("choice criteria must map options")?;
    if !(2..=255).contains(&map.len()) {
        return Err(format!("choice needs 2-255 options, got {}", map.len()));
    }
    if map.keys().any(|option| option.is_empty()) {
        return Err("choice options must be non-empty".into());
    }
    Ok(Declared::Choice(map.keys().cloned().collect()))
}

fn validate_score(criteria: Option<&Value>) -> Result<Declared, String> {
    let levels = criteria.and_then(Value::as_array).ok_or("score criteria must list levels")?;
    if !(2..=10).contains(&levels.len()) {
        return Err(format!("score needs 2-10 levels, got {}", levels.len()));
    }
    Ok(Declared::Score(levels.len()))
}

#[cfg(test)]
#[path = "question_tests.rs"]
mod tests;
