// One-source category rules, CLI/effort boundaries, and independent scaling.
// Covers all categories using captured relay models and exact harness rows.
// Deps: evidence_from, rescale, test_support; no network.

use super::test_support::{live, sample};
use super::*;

const OPUS: &str = "anthropic/claude-opus-5.5";

#[test]
fn category_rules_use_one_board_and_never_blend() {
    let feed = live();
    for category in [
        TaskCategory::SimpleEdit,
        TaskCategory::ComplexImpl,
        TaskCategory::Debugging,
        TaskCategory::Testing,
        TaskCategory::Refactoring,
    ] {
        let evidence = evidence_from(&feed, OPUS, "claude", Some("max"), category);
        assert!((evidence.capability.expect("accuracy") - 6.485).abs() < 1e-10);
        assert_eq!(
            evidence.selected.expect("selected").source,
            "terminal_bench"
        );
    }
    for category in [TaskCategory::Research, TaskCategory::Documentation] {
        let evidence = evidence_from(&feed, OPUS, "claude", Some("max"), category);
        assert_eq!(evidence.capability, Some(10.0), "{category:?}");
        assert_eq!(evidence.selected.expect("selected").board, "eci");
    }
    let frontend = evidence_from(&feed, OPUS, "claude", Some("max"), TaskCategory::Frontend);
    assert_eq!(frontend.capability, Some(10.0));
    assert_eq!(frontend.selected.expect("selected").source, "lmarena");
}

#[test]
fn terminal_bench_requires_exact_cli_model_and_effort_else_harness_unmeasured() {
    let feed = live();
    for (cli, effort) in [
        ("codex", Some("max")),
        ("claude", Some("low")),
        ("claude", None),
    ] {
        let evidence = evidence_from(&feed, OPUS, cli, effort, TaskCategory::Testing);
        assert_eq!(evidence.capability, Some(10.0));
        assert_eq!(evidence.harness, "harness unmeasured");
        assert_eq!(evidence.selected.expect("fallback").source, "epoch");
    }
    let other = evidence_from(
        &feed,
        "openai/gpt-4o-mini",
        "claude",
        Some("max"),
        TaskCategory::Testing,
    );
    assert_eq!(other.capability, Some(0.0));
    assert_eq!(other.harness, "harness unmeasured");
    let exact = evidence_from(&feed, OPUS, "claude", Some("max"), TaskCategory::Testing);
    assert!((exact.capability.expect("accuracy") - 6.485).abs() < 1e-10);
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
        evidence_from(
            &feed,
            "302ai/kimi-k2-thinking",
            "codex",
            None,
            TaskCategory::Research
        )
        .capability,
        None
    );
    assert_eq!(
        evidence_from(
            &feed,
            "302ai/kimi-k2-thinking",
            "codex",
            None,
            TaskCategory::Frontend
        )
        .capability,
        None
    );
}

#[test]
fn degenerate_source_range_is_unknown_and_accuracy_uses_its_own_range() {
    let mut feed = sample();
    feed.models[3].scores.clear();
    assert_eq!(
        evidence_from(
            &feed,
            "302ai/kimi-k2-thinking",
            "codex",
            None,
            TaskCategory::Research
        )
        .capability,
        None
    );
    let mut score = live().models[0].scores[0].clone();
    score.value = 0.72;
    score.unit = "rate".into();
    assert_eq!(rescale(&feed, &score), Some(7.199999999999999));
    score.value = 1.1;
    assert_eq!(rescale(&feed, &score), None);
}

#[test]
fn webdev_prefers_configured_effort_else_highest_raw_value() {
    let feed = live();
    let id = "anthropic/claude-sonnet-5.5";
    for (effort, selected_effort) in [
        (Some("high"), "high"),
        (None, "xhigh"),
        (Some("unmeasured"), "xhigh"),
    ] {
        let evidence = evidence_from(&feed, id, "cursor", effort, TaskCategory::Frontend);
        let score = evidence.selected.expect("webdev");
        let captured = feed.models[1]
            .scores
            .iter()
            .find(|row| {
                row.source == "lmarena"
                    && row.board == "webdev"
                    && row.effort.as_deref() == Some(selected_effort)
            })
            .expect("captured webdev row");
        assert_eq!(
            &score, captured,
            "preserve the exact captured value and metadata"
        );
        let expected =
            10.0 * (score.value - 1446.5703748835622) / (1813.6003351106383 - 1446.5703748835622);
        assert!((evidence.capability.expect("webdev range") - expected).abs() < 1e-10);
        assert_eq!(evidence.harness, "harness unmeasured");
    }
}
