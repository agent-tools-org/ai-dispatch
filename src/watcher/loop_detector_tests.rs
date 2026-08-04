// Loop detector tests for repetition counts, raw keys, and injected wall-clock timing.
// Verifies fast bursts survive while sustained tool activity still triggers protection.

use super::progress::LoopDetector;
use crate::types::EventKind;
use std::cell::Cell;
use std::rc::Rc;
use std::time::{Duration, Instant};

type TestDetector = LoopDetector<Box<dyn Fn() -> Instant>>;

fn timed_detector() -> (TestDetector, Rc<Cell<Duration>>) {
    let elapsed = Rc::new(Cell::new(Duration::ZERO));
    let clock_elapsed = Rc::clone(&elapsed);
    let base = Instant::now();
    let clock: Box<dyn Fn() -> Instant> = Box::new(move || base + clock_elapsed.get());
    (LoopDetector::with_clock(clock), elapsed)
}

fn advance(elapsed: &Cell<Duration>, seconds: u64) {
    elapsed.set(elapsed.get() + Duration::from_secs(seconds));
}

fn push_events<I, S>(detector: &mut TestDetector, elapsed: &Cell<Duration>, events: I)
where
    I: IntoIterator<Item = S>,
    S: AsRef<str>,
{
    for detail in events {
        detector.push(detail.as_ref(), EventKind::ToolCall, None);
        advance(elapsed, 20);
    }
}

fn push_tool_call(detector: &mut TestDetector, elapsed: &Cell<Duration>, raw_key: &str) {
    detector.push("/bin/zsh -lc \"nl -ba .../aggregat...", EventKind::ToolCall, Some(raw_key));
    advance(elapsed, 20);
}

#[test]
fn loop_detector_patterns() {
    let cases: Vec<(bool, Vec<&str>)> = vec![
        (false, vec!["a", "b", "c", "d", "e", "f", "g", "h", "i", "j"]),
        (true, vec!["repeat"; 10]),
        (false, vec!["dup", "dup", "dup", "dup", "dup", "dup", "dup", "u1", "u2", "u3"]),
        (true, vec!["dup", "dup", "dup", "dup", "dup", "dup", "dup", "dup", "u1", "u2"]),
    ];
    for (expected, events) in cases {
        let (mut detector, elapsed) = timed_detector();
        push_events(&mut detector, &elapsed, events);
        assert_eq!(detector.is_looping(), expected);
    }
}

#[test]
fn loop_detector_does_not_kill_fast_burst() {
    let (mut detector, _elapsed) = timed_detector();
    for _ in 0..10 {
        detector.push("same command", EventKind::ToolCall, None);
    }
    assert!(!detector.is_looping());
}

#[test]
fn loop_detector_kills_sustained_repeat() {
    let (mut detector, elapsed) = timed_detector();
    push_events(&mut detector, &elapsed, std::iter::repeat_n("same command", 10));
    assert!(detector.is_looping());
}

#[test]
fn loop_detector_kills_fast_loop_after_duration_threshold() {
    let (mut detector, elapsed) = timed_detector();
    for _ in 0..120 {
        detector.push("same command", EventKind::ToolCall, None);
        assert!(!detector.is_looping());
        advance(&elapsed, 1);
    }
    detector.push("same command", EventKind::ToolCall, None);
    assert!(detector.is_looping());
}

#[test]
fn loop_detector_counts_format_and_lint_as_evidence() {
    for kind in [EventKind::Format, EventKind::Lint] {
        let (mut detector, elapsed) = timed_detector();
        for _ in 0..10 {
            detector.push("same command", kind, None);
            advance(&elapsed, 20);
        }
        assert!(detector.is_looping(), "{kind:?} should count as loop evidence");
    }
}

#[test]
fn loop_detector_never_kills_pure_narration() {
    let (mut detector, elapsed) = timed_detector();
    for _ in 0..20 {
        detector.push("I am still considering the approach", EventKind::Reasoning, None);
        advance(&elapsed, 20);
    }
    assert!(!detector.is_looping());
}

#[test]
fn loop_detector_ignores_empty_details() {
    let (mut detector, elapsed) = timed_detector();
    push_events(&mut detector, &elapsed, ["", "  ", "\t"].repeat(7));
    assert!(!detector.is_looping());
}

#[test]
fn loop_detector_distinguishes_long_details() {
    let shared_prefix = "Read(".to_string() + &"a".repeat(110);
    let first = format!("{shared_prefix}file1.rs)");
    let second = format!("{shared_prefix}file2.rs)");
    let events = std::iter::repeat_n(first.as_str(), 5).chain(std::iter::repeat_n(second.as_str(), 5));
    let (mut detector, elapsed) = timed_detector();
    push_events(&mut detector, &elapsed, events);
    assert!(!detector.is_looping());
}

#[test]
fn loop_detector_uses_file_write_raw_path_with_higher_threshold() {
    let (mut detector, elapsed) = timed_detector();
    for index in 0..20 {
        let raw_key = format!("/tmp/worktree/evaluator-different-{index}.rs");
        detector.push(".../evaluator-d...", EventKind::FileWrite, Some(&raw_key));
        advance(&elapsed, 10);
    }
    assert!(!detector.is_looping());

    let (mut detector, elapsed) = timed_detector();
    for _ in 0..14 {
        detector.push(".../evaluator-d...", EventKind::FileWrite, Some("/tmp/worktree/evaluator.rs"));
        advance(&elapsed, 10);
    }
    assert!(!detector.is_looping());
    detector.push(".../evaluator-d...", EventKind::FileWrite, Some("/tmp/worktree/evaluator.rs"));
    assert!(detector.is_looping());
}

#[test]
fn loop_detector_uses_tool_call_raw_command() {
    let (mut detector, elapsed) = timed_detector();
    for index in 0..10 {
        let raw_key = format!("/bin/zsh -lc \"nl -ba filler-{index}/src/lib.rs\"");
        push_tool_call(&mut detector, &elapsed, &raw_key);
    }
    assert!(!detector.is_looping());
}

#[test]
fn loop_detector_still_flags_repeated_tool_call_raw_command() {
    let (mut detector, elapsed) = timed_detector();
    for _ in 0..10 {
        push_tool_call(&mut detector, &elapsed, "/bin/zsh -lc \"nl -ba filler/src/lib.rs\"");
    }
    assert!(detector.is_looping());
}

#[test]
fn loop_detector_resets_file_write_counter_on_non_file_event() {
    let (mut detector, elapsed) = timed_detector();
    for _ in 0..14 {
        detector.push("file.rs", EventKind::FileWrite, Some("/tmp/file.rs"));
        advance(&elapsed, 10);
    }
    detector.push("thinking", EventKind::Reasoning, None);
    detector.push("file.rs", EventKind::FileWrite, Some("/tmp/file.rs"));
    assert!(!detector.is_looping());
}
