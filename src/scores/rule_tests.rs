// One-source category rules, CLI/effort boundaries, and independent scaling.
// Covers all categories using the real relay sample plus explicit harness rows.
// Deps: evidence_from, rescale, test_support; no network.

use super::test_support::{add_terminal, sample};
use super::*;

const KIMI: &str = "302ai/kimi-k2-thinking";

#[test]
fn category_rules_use_one_board_and_never_blend() {
    let mut feed = sample();
    add_terminal(&mut feed, "codex", Some("high"));
    let webdev = feed.models[1].scores[0].clone();
    feed.models[2].scores.push(webdev);
    for category in [
        TaskCategory::SimpleEdit,
        TaskCategory::ComplexImpl,
        TaskCategory::Debugging,
        TaskCategory::Testing,
        TaskCategory::Refactoring,
    ] {
        let evidence = evidence_from(&feed, KIMI, "codex", Some("high"), category);
        assert_eq!(evidence.capability, Some(7.2), "{category:?}");
        assert_eq!(
            evidence.selected.expect("selected").source,
            "terminal_bench"
        );
    }
    for category in [TaskCategory::Research, TaskCategory::Documentation] {
        let evidence = evidence_from(&feed, KIMI, "codex", Some("high"), category);
        assert_eq!(evidence.capability, Some(10.0), "{category:?}");
        assert_eq!(evidence.selected.expect("selected").board, "eci");
    }
    let frontend = evidence_from(&feed, KIMI, "codex", Some("high"), TaskCategory::Frontend);
    assert_eq!(frontend.capability, Some(0.0));
    assert_eq!(frontend.selected.expect("selected").source, "lmarena");
}

#[test]
fn terminal_bench_requires_exact_cli_model_and_effort_else_harness_unmeasured() {
    let mut feed = sample();
    add_terminal(&mut feed, "codex", Some("high"));
    for (cli, effort) in [
        ("claude", Some("high")),
        ("codex", Some("low")),
        ("codex", None),
    ] {
        let evidence = evidence_from(&feed, KIMI, cli, effort, TaskCategory::Testing);
        assert_eq!(evidence.capability, Some(10.0));
        assert_eq!(evidence.harness, "harness unmeasured");
        assert_eq!(evidence.selected.expect("fallback").source, "epoch");
    }
    let other = evidence_from(
        &feed,
        "302ai/qwen3-30b-a3b",
        "codex",
        Some("high"),
        TaskCategory::Testing,
    );
    assert_eq!(other.capability, Some(0.0));
    assert_eq!(other.harness, "harness unmeasured");
    let exact = evidence_from(&feed, KIMI, "codex", Some("high"), TaskCategory::Testing);
    assert_eq!(exact.capability, Some(7.2));
    assert_eq!(exact.harness, "measured");
}

#[test]
fn unrelated_board_failed_source_unmapped_or_absent_model_is_unknown() {
    let mut feed = sample();
    for id in ["302ai/gemini-3-pro-preview", "Baichuan 2-7B", "missing"] {
        assert_eq!(
            evidence_from(&feed, id, "codex", None, TaskCategory::Research).capability,
            None
        );
    }
    feed.sources.get_mut("epoch").expect("epoch").ok = false;
    assert_eq!(
        evidence_from(&feed, KIMI, "codex", None, TaskCategory::Research).capability,
        None
    );
    assert_eq!(
        evidence_from(&feed, KIMI, "codex", None, TaskCategory::Frontend).capability,
        None
    );
}

#[test]
fn degenerate_source_range_is_unknown_and_accuracy_uses_its_own_range() {
    let mut feed = sample();
    feed.models[3].scores.clear();
    assert_eq!(
        evidence_from(&feed, KIMI, "codex", None, TaskCategory::Research).capability,
        None
    );
    add_terminal(&mut feed, "codex", None);
    let mut score = feed.models[2].scores.last().expect("terminal").clone();
    score.value = 0.72;
    score.unit = "rate".into();
    assert_eq!(rescale(&feed, &score), Some(7.199999999999999));
    score.value = 1.1;
    assert_eq!(rescale(&feed, &score), None);
}
