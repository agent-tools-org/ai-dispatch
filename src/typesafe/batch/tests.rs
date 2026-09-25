// Batch JSONL preflight, resume, append safety, and summary contract tests.
// Uses only temporary files and in-memory JSON; no network or keychain.

use super::*;
use serde_json::json;
use std::io::Write;

#[test]
fn parses_text_and_json_without_changing_state() {
    let input = b"{\"id\":\"text\",\"state\":\"hello\"}\n{\"id\":\"json\",\"state\":{\"a\":[1]}}\n";
    let items = read_items(&input[..]).expect("items");
    assert_eq!(items[0].state, ClassifyState::Text("hello".into()));
    assert_eq!(items[1].state, ClassifyState::Json(json!({"a": [1]})));
    let scalar = read_items(&b"{\"id\":\"scalar\",\"state\":null}"[..]).expect("defer screening");
    assert_eq!(scalar[0].state, ClassifyState::Json(Value::Null));
    assert!(read_items(&b""[..]).expect("empty").is_empty());
}

#[test]
fn rejects_missing_duplicate_or_invalid_ids_and_malformed_lines() {
    for input in [
        r#"{"state":"private state"}"#,
        r#"{"id":7,"state":"x"}"#,
        r#"{"id":" ","state":"x"}"#,
        r#"{"id":"a"}"#,
        "[]",
        "\n",
        "broken",
        "{\"id\":\"a\",\"state\":1}\n{\"id\":\"a\",\"state\":2}",
    ] {
        let error = read_items(input.as_bytes()).expect_err(input);
        assert_eq!(error.exit_code(), 4);
        assert!(!error.message.contains("private state"));
    }
}

#[test]
fn resume_skips_any_prior_success_but_not_failures() {
    let dir = tempfile::tempdir().expect("tempdir");
    let path = dir.path().join("out");
    assert!(successful_ids(&path).expect("missing").is_empty());
    std::fs::write(
        &path,
        concat!(
            "{\"id\":\"a\",\"ok\":true}\n",
            "{\"id\":\"a\",\"ok\":false}\n",
            "{\"id\":\"b\",\"ok\":false}\n",
            "{\"id\":\"c\",\"ok\":true}"
        ),
    )
    .expect("write");
    let ids = successful_ids(&path).expect("resume");
    assert_eq!(ids.len(), 2);
    assert!(ids.contains("a") && ids.contains("c") && !ids.contains("b"));
    for malformed in ["{", "{\"id\":\"a\"}", "{\"id\":3,\"ok\":true}"] {
        std::fs::write(&path, malformed).expect("write");
        assert_eq!(successful_ids(&path).expect_err("malformed").exit_code(), 4);
    }
}

#[test]
fn append_preserves_rows_and_separates_unterminated_final_line() {
    let dir = tempfile::tempdir().expect("tempdir");
    let input = dir.path().join("in");
    let output = dir.path().join("out");
    std::fs::write(&input, "input").expect("input");
    std::fs::write(&output, "{\"id\":\"a\",\"ok\":true}").expect("output");
    let mut file = open_output(&input, &output).expect("append");
    writeln!(file, "{{\"id\":\"b\",\"ok\":false}}").expect("write");
    assert_eq!(std::fs::read_to_string(&output).expect("read").lines().count(), 2);
    assert!(open_output(&input, &input).is_err());
    let alias = dir.path().join("alias");
    std::fs::hard_link(&input, &alias).expect("hardlink");
    assert!(open_output(&input, &alias).is_err());
    assert_eq!(std::fs::read_to_string(&input).expect("read"), "input");
}

#[test]
fn summary_counts_each_choice_and_includes_zero_options() {
    let declared = BTreeMap::from([
        ("v".into(), Declared::Choice(vec!["SHIP".into(), "FIX".into()])),
        ("lang".into(), Declared::Choice(vec!["rust".into(), "go".into()])),
        ("n".into(), Declared::Noul),
    ]);
    let mut summary = Summary::new(4, 1, &declared);
    for choice in ["SHIP", "FIX"] {
        summary.record(&Ok(
            json!({"answers": {"v": {"choice": choice}, "lang": {"choice": "rust"}}}),
        ));
    }
    summary.record(&Err(invalid("bad")));
    assert_eq!(
        (summary.total, summary.ok, summary.failed, summary.skipped),
        (4, 2, 1, 1)
    );
    assert_eq!(
        summary.choices["v"],
        BTreeMap::from([("FIX".into(), 1), ("SHIP".into(), 1)])
    );
    assert_eq!(summary.choices["lang"]["go"], 0);
    assert_eq!(summary.choices["lang"]["rust"], 2);
    assert_eq!(summary.exit_code(), 6);
    assert!(summary.to_string().contains("total=4 ok=2 failed=1 skipped=1"));
}
