// Validation of a TypeSafe System One response against the questions that were asked.
// Exports: validate_response. Only declared ids, options, and levels pass; numbers must be finite.
// Deps: serde_json; question::Declared.

use super::question::Declared;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

/// `{"model", "answers", "usage": {"input_tokens"}}` from a 2xx body, or a reason it was refused.
/// Reasons never echo response content.
pub(crate) fn validate_response(body: &str, declared: &BTreeMap<String, Declared>) -> Result<Value, String> {
    let response: Value = serde_json::from_str(body).map_err(|_| "body is not JSON".to_string())?;
    let model = response.get("model").and_then(Value::as_str).ok_or("model is missing")?;
    if !is_jev_version(model) {
        return Err("model is not jev-<version>".into());
    }
    let answers = response.get("answers").and_then(Value::as_object).ok_or("answers is missing")?;
    if answers.keys().any(|id| !declared.contains_key(id)) {
        return Err("an answer id was not asked".into());
    }
    let mut validated = Map::new();
    for (id, expected) in declared {
        let answer = answers.get(id).ok_or_else(|| format!("answer '{id}' is missing"))?;
        let checked = validate_answer(answer, expected).map_err(|err| format!("answer '{id}': {err}"))?;
        validated.insert(id.clone(), checked);
    }
    let input_tokens = match response.get("usage").and_then(|usage| usage.get("input_tokens")) {
        None | Some(Value::Null) => Value::Null,
        Some(tokens) => json!(tokens.as_u64().ok_or("usage.input_tokens is not a count")?),
    };
    Ok(json!({ "model": model, "answers": validated, "usage": { "input_tokens": input_tokens } }))
}

fn is_jev_version(model: &str) -> bool {
    let Some(version) = model.strip_prefix("jev-") else {
        return false;
    };
    model.len() <= 32 && version.split('.').all(|part| !part.is_empty() && part.chars().all(|c| c.is_ascii_digit()))
}

fn validate_answer(answer: &Value, expected: &Declared) -> Result<Value, String> {
    let object = answer.as_object().ok_or("not an object")?;
    let kind = object.get("type").and_then(Value::as_str).ok_or("type is missing")?;
    match (expected, kind) {
        (Declared::Noul, "noul") => {
            let value = unit(object.get("noul"), "noul")?.ok_or("noul is missing")?;
            Ok(json!({ "type": "noul", "noul": value }))
        }
        (Declared::Choice(options), "choice") => {
            let choice = object.get("choice").and_then(Value::as_str).ok_or("choice is missing")?;
            if !options.iter().any(|option| option == choice) {
                return Err("choice is not a declared option".into());
            }
            let mut out = json!({ "type": "choice", "choice": choice });
            add_distribution(&mut out, object, options)?;
            Ok(out)
        }
        (Declared::Score(levels), "score") => {
            let score = finite(object.get("score"), "score")?.ok_or("score is missing")?;
            let top = (*levels - 1) as f64;
            if !(0.0..=top).contains(&score) {
                return Err("score is outside the declared levels".into());
            }
            let keys: Vec<String> = (0..*levels).map(|level| level.to_string()).collect();
            let mut out = json!({ "type": "score", "score": score });
            add_distribution(&mut out, object, &keys)?;
            Ok(out)
        }
        _ => Err("type does not match the question".into()),
    }
}

/// Optional `confidence` and `probabilities`, kept only when valid.
fn add_distribution(out: &mut Value, answer: &Map<String, Value>, keys: &[String]) -> Result<(), String> {
    if let Some(confidence) = unit(answer.get("confidence"), "confidence")? {
        out["confidence"] = json!(confidence);
    }
    match answer.get("probabilities") {
        None | Some(Value::Null) => Ok(()),
        Some(Value::Object(map)) => {
            let mut checked = Map::new();
            for (key, value) in map {
                if !keys.contains(key) {
                    return Err("probabilities name an undeclared option".into());
                }
                let probability = unit(Some(value), "probability")?.ok_or("probability is null")?;
                checked.insert(key.clone(), json!(probability));
            }
            out["probabilities"] = Value::Object(checked);
            Ok(())
        }
        Some(_) => Err("probabilities is not a map".into()),
    }
}

fn finite(value: Option<&Value>, name: &str) -> Result<Option<f64>, String> {
    match value {
        None | Some(Value::Null) => Ok(None),
        Some(number) => match number.as_f64() {
            Some(value) if value.is_finite() => Ok(Some(value)),
            _ => Err(format!("{name} is not a finite number")),
        },
    }
}

fn unit(value: Option<&Value>, name: &str) -> Result<Option<f64>, String> {
    let value = finite(value, name)?;
    if value.is_some_and(|value| !(0.0..=1.0).contains(&value)) {
        return Err(format!("{name} is outside 0..1"));
    }
    Ok(value)
}

#[cfg(test)]
#[path = "answer_tests.rs"]
mod tests;
