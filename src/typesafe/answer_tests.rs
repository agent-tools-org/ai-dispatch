// Tests: TypeSafe response validation, including hostile models, options, and probability keys.
// Exports: none.
// Deps: super answer helpers.

use super::*;

fn declared() -> BTreeMap<String, Declared> {
    BTreeMap::from([
        ("urgent".to_string(), Declared::Noul),
        ("team".to_string(), Declared::Choice(vec!["billing".into(), "tech".into()])),
        ("mood".to_string(), Declared::Score(3)),
    ])
}

fn good() -> Value {
    json!({
        "model": "jev-1.13.0",
        "answers": {
            "urgent": { "type": "noul", "noul": 0.95 },
            "team": { "type": "choice", "choice": "billing",
                      "probabilities": { "billing": 0.88, "tech": 0.12 }, "confidence": 0.81 },
            "mood": { "type": "score", "score": 1.05, "legend": { "0": "a", "1": "b", "2": "c" },
                      "probabilities": { "0": 0.0, "1": 0.95, "2": 0.05 }, "confidence": 0.92 }
        },
        "usage": { "input_tokens": 296, "output_tokens": 20 }
    })
}

fn check(response: Value) -> Result<Value, String> {
    validate_response(&response.to_string(), &declared())
}

#[test]
fn valid_response_passes_answer_fields_through() {
    let out = check(good()).expect("valid");
    assert_eq!(out["model"], "jev-1.13.0");
    assert_eq!(out["usage"], json!({ "input_tokens": 296 }));
    assert_eq!(out["answers"]["urgent"], json!({ "type": "noul", "noul": 0.95 }));
    assert_eq!(out["answers"]["team"]["choice"], "billing");
    assert_eq!(out["answers"]["team"]["confidence"], 0.81);
    assert_eq!(out["answers"]["mood"]["probabilities"]["1"], 0.95);
    assert!(out["answers"]["mood"].get("legend").is_none(), "only the documented fields leave");
}

#[test]
fn hostile_or_malformed_models_are_rejected() {
    for model in [json!("jev-1.13.0\"; rm -rf /"), json!("gpt-4"), json!("jev-"), json!("jev-1..2"),
        json!("jev-latest"), json!(7), json!(format!("jev-{}", "1".repeat(40)))] {
        let mut response = good();
        response["model"] = model.clone();
        assert_eq!(check(response).unwrap_err(), "model is not jev-<version>", "{model}");
    }
}

#[test]
fn undeclared_option_or_probability_key_is_rejected() {
    let mut response = good();
    response["answers"]["team"]["choice"] = json!("sales");
    assert!(check(response).unwrap_err().contains("not a declared option"));
    let mut response = good();
    response["answers"]["team"]["probabilities"]["<script>"] = json!(0.0);
    let err = check(response).unwrap_err();
    assert!(err.contains("undeclared option") && !err.contains("script"), "{err}");
    let mut response = good();
    response["answers"]["mood"]["probabilities"]["3"] = json!(0.0);
    assert!(check(response).is_err());
}

#[test]
fn out_of_range_or_wrong_typed_numbers_are_rejected() {
    let cases = [
        ("/answers/urgent/noul", json!(1.5)),
        ("/answers/urgent/noul", json!("0.9")),
        ("/answers/team/confidence", json!(-0.1)),
        ("/answers/mood/score", json!(2.5)),
        ("/answers/mood/probabilities/0", json!(null)),
        ("/usage/input_tokens", json!(-3)),
    ];
    for (pointer, value) in cases {
        let mut response = good();
        *response.pointer_mut(pointer).expect(pointer) = value;
        assert!(check(response).is_err(), "{pointer}");
    }
    assert!(validate_response("{\"model\":\"jev-1\",\"answers\":{\"urgent\":{\"type\":\"noul\",\"noul\":1e400}}}",
        &BTreeMap::from([("urgent".to_string(), Declared::Noul)])).is_err());
}

#[test]
fn missing_extra_or_mistyped_answers_are_rejected() {
    let mut response = good();
    response["answers"].as_object_mut().expect("answers").remove("mood");
    assert!(check(response).unwrap_err().contains("missing"));
    let mut response = good();
    response["answers"]["extra"] = json!({ "type": "noul", "noul": 0.5 });
    assert!(check(response).unwrap_err().contains("not asked"));
    let mut response = good();
    response["answers"]["urgent"] = json!({ "type": "score", "score": 1.0 });
    assert!(check(response).unwrap_err().contains("does not match"));
    assert!(validate_response("not json", &declared()).is_err());
}

#[test]
fn missing_usage_is_null_not_invented() {
    let mut response = good();
    response.as_object_mut().expect("object").remove("usage");
    assert_eq!(check(response).expect("valid")["usage"]["input_tokens"], Value::Null);
}
