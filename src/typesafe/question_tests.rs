// Tests: flag pairing, merge with a native questions map, and every question validation error.
// Exports: none.
// Deps: super question helpers.

use super::*;

fn ev(kind: FlagKind, value: &str) -> FlagEvent {
    FlagEvent { kind, value: value.to_string() }
}

fn map(value: Value) -> Map<String, Value> {
    value.as_object().expect("object").clone()
}

#[test]
fn each_choice_pairs_with_the_next_options() {
    let events = [
        ev(FlagKind::Choice, "verdict=What is the verdict?"),
        ev(FlagKind::Noul, "tests=Shows test output?"),
        ev(FlagKind::Options, "SHIP,FIX,BLOCK"),
        ev(FlagKind::Choice, "lang=Which language?"),
        ev(FlagKind::Options, "rust, go"),
        ev(FlagKind::Score, "risk=How risky?"),
        ev(FlagKind::Levels, "low,mid,high"),
    ];
    let built: BTreeMap<String, Value> = questions_from_flags(&events).expect("pairs").into_iter().collect();
    assert_eq!(built["verdict"]["criteria"], json!({ "SHIP": null, "FIX": null, "BLOCK": null }));
    assert_eq!(built["lang"]["criteria"], json!({ "rust": null, "go": null }));
    assert_eq!(built["tests"], json!({ "type": "noul", "instructions": "Shows test output?" }));
    assert_eq!(built["risk"]["criteria"], json!(["low", "mid", "high"]));
    assert_eq!(built["risk"]["type"], "score");
}

#[test]
fn question_text_may_contain_equals_signs() {
    let built = questions_from_flags(&[ev(FlagKind::Noul, "q=Is a=b here?")]).expect("noul");
    assert_eq!(built[0].0, "q");
    assert_eq!(built[0].1["instructions"], "Is a=b here?");
}

#[test]
fn unpaired_flags_are_refused() {
    let cases: [&[FlagEvent]; 5] = [
        &[ev(FlagKind::Choice, "a=A?")],
        &[ev(FlagKind::Choice, "a=A?"), ev(FlagKind::Choice, "b=B?"), ev(FlagKind::Options, "x,y")],
        &[ev(FlagKind::Options, "x,y")],
        &[ev(FlagKind::Score, "a=A?"), ev(FlagKind::Options, "x,y")],
        &[ev(FlagKind::Choice, "a=A?"), ev(FlagKind::Options, "x,y"), ev(FlagKind::Options, "p,q")],
    ];
    for events in cases {
        assert!(questions_from_flags(events).is_err(), "{events:?}");
    }
}

#[test]
fn malformed_flag_values_are_refused() {
    assert!(questions_from_flags(&[ev(FlagKind::Noul, "no-equals")]).is_err());
    assert!(questions_from_flags(&[ev(FlagKind::Noul, "a=  ")]).is_err());
    let empty_option = [ev(FlagKind::Choice, "a=A?"), ev(FlagKind::Options, "x,,y")];
    assert!(questions_from_flags(&empty_option).is_err());
    let repeated = [ev(FlagKind::Choice, "a=A?"), ev(FlagKind::Options, "x,y,x")];
    assert!(questions_from_flags(&repeated).unwrap_err().contains("repeats"));
}

#[test]
fn merge_keeps_file_entries_and_refuses_duplicate_ids() {
    let file = map(json!({ "from_file": { "type": "noul", "instructions": "File?" } }));
    let flags = vec![("from_flag".to_string(), json!({ "type": "noul", "instructions": "Flag?" }))];
    let merged = merge_questions(Some(file.clone()), flags).expect("merge");
    assert_eq!(merged.len(), 2);
    let clash = vec![("from_file".to_string(), json!({ "type": "noul", "instructions": "Again?" }))];
    assert!(merge_questions(Some(file), clash).unwrap_err().contains("duplicate question id"));
    let twice = vec![
        ("x".to_string(), json!({ "type": "noul", "instructions": "1?" })),
        ("x".to_string(), json!({ "type": "noul", "instructions": "2?" })),
    ];
    assert!(merge_questions(None, twice).is_err());
}

#[test]
fn validation_returns_declared_shapes() {
    let declared = validate_questions(&map(json!({
        "n": { "type": "noul", "instructions": "Yes?", "criteria": { "true": "yes", "false": "no" } },
        "c": { "type": "choice", "instructions": { "question": "Which?" }, "criteria": { "a": null, "b": "B" } },
        "s": { "type": "score", "instructions": "How much?", "criteria": ["low", "high"] }
    })))
    .expect("valid");
    assert_eq!(declared["n"], Declared::Noul);
    assert_eq!(declared["c"], Declared::Choice(vec!["a".into(), "b".into()]));
    assert_eq!(declared["s"], Declared::Score(2));
}

#[test]
fn each_validation_error_is_reported() {
    let options = |count: usize| -> Value {
        Value::Object((0..count).map(|index| (format!("o{index}"), Value::Null)).collect())
    };
    let levels = |count: usize| json!(vec!["level"; count]);
    let cases = [
        (json!({}), "at least one question"),
        (json!({ "bad id!": { "type": "noul", "instructions": "?" } }), "question ids"),
        (json!({ "a": "not an object" }), "must be an object"),
        (json!({ "a": { "type": "noul", "instructions": "?", "extra": 1 } }), "unknown field"),
        (json!({ "a": { "type": "noul", "instructions": "" } }), "instructions"),
        (json!({ "a": { "type": "vote", "instructions": "?" } }), "type must be"),
        (json!({ "a": { "type": "noul", "instructions": "?", "criteria": { "maybe": "x" } } }), "noul criteria"),
        (json!({ "a": { "type": "choice", "instructions": "?", "criteria": options(1) } }), "2-255 options"),
        (json!({ "a": { "type": "choice", "instructions": "?", "criteria": options(256) } }), "2-255 options"),
        (json!({ "a": { "type": "choice", "instructions": "?", "criteria": ["x", "y"] } }), "map options"),
        (json!({ "a": { "type": "score", "instructions": "?", "criteria": levels(1) } }), "2-10 levels"),
        (json!({ "a": { "type": "score", "instructions": "?", "criteria": levels(11) } }), "2-10 levels"),
    ];
    for (questions, expected) in cases {
        let err = validate_questions(&map(questions.clone())).expect_err(expected);
        assert!(err.contains(expected), "{questions}: {err}");
    }
    let limits = json!({
        "c": { "type": "choice", "instructions": "?", "criteria": options(255) },
        "s": { "type": "score", "instructions": "?", "criteria": levels(10) }
    });
    assert!(validate_questions(&map(limits)).is_ok());
}
