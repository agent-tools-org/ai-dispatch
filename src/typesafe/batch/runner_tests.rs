// Injected batch classifiers exercise concurrency, retries, isolation, and output failure.
// No network, keychain, or environment changes; synchronization uses std primitives.

use super::*;
use crate::typesafe::classify::{ClassifyState, DEFAULT_MODEL};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};

fn template() -> ClassifyRequest {
    ClassifyRequest {
        state: ClassifyState::Text("template".into()),
        questions: json!({"v": {"type": "choice", "instructions": "Verdict?", "criteria": {"yes": null, "no": null}}})
            .as_object()
            .expect("questions")
            .clone(),
        model: DEFAULT_MODEL.into(),
        timeout_secs: 20,
        allow_secret_like: false,
    }
}

fn items(count: usize) -> Vec<Item> {
    (0..count)
        .map(|id| Item {
            id: id.to_string(),
            state: ClassifyState::Text(id.to_string()),
        })
        .collect()
}

fn answer() -> Attempt {
    Ok(
        json!({"answers": {"v": {"type": "choice", "choice": "yes"}}, "model": DEFAULT_MODEL,
        "usage": {"input_tokens": 1}, "latency_ms": 0}),
    )
}

#[test]
fn workers_reach_but_never_exceed_the_concurrency_limit() {
    for jobs in [1, 4, 16] {
        let active = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let barrier = Barrier::new(jobs);
        let summary = run(items(jobs * 2), &template(), jobs, 0, &mut Vec::new(), &|_| {
            let current = active.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(current, Ordering::SeqCst);
            barrier.wait();
            active.fetch_sub(1, Ordering::SeqCst);
            answer()
        })
        .expect("batch");
        assert_eq!(peak.load(Ordering::SeqCst), jobs);
        assert_eq!(summary.ok, jobs * 2);
    }
}

#[test]
fn failed_and_refused_items_do_not_stop_siblings_or_leak_state() {
    let calls = AtomicUsize::new(0);
    let mut input = items(5);
    input[1].state = ClassifyState::Text("sk-proj-A1b2C3d4E5f6G7h8".into());
    input[2].state = ClassifyState::Text("x".repeat(100_001));
    input[3].state = ClassifyState::Json(json!(null));
    let mut output = Vec::new();
    let summary = run(input, &template(), 4, 2, &mut output, &|request| {
        calls.fetch_add(1, Ordering::SeqCst);
        if request.state == ClassifyState::Text("4".into()) {
            Err((ClassifyError::new(ErrorKind::Api, "network failed"), false))
        } else {
            answer()
        }
    })
    .expect("batch");
    assert_eq!(
        (summary.total, summary.ok, summary.failed, summary.skipped),
        (7, 1, 4, 2)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 2);
    let text = String::from_utf8(output).expect("utf8");
    assert!(!text.contains("A1b2C3"));
    let rows: Vec<Value> = text
        .lines()
        .map(|line| serde_json::from_str(line).expect("json"))
        .collect();
    assert_eq!(rows.len(), 5);
    assert_eq!(
        rows.iter().find(|row| row["id"] == "1").expect("refused")["error_kind"],
        "refused"
    );
    assert_eq!(summary.choices["v"]["yes"], 1);
}

#[test]
fn throttling_retries_more_than_once_and_slows_later_items_too() {
    let calls = Mutex::new(Vec::new());
    let mut output = Vec::new();
    let base = Duration::from_millis(5);
    let summary = run_with_gate(
        items(2),
        &template(),
        1,
        0,
        &mut output,
        &|_| {
            let mut calls = calls.lock().expect("calls");
            calls.push(Instant::now());
            if calls.len() <= 2 {
                Err((ClassifyError::new(ErrorKind::Api, "HTTP 429"), true))
            } else {
                answer()
            }
        },
        Gate::new(base),
    )
    .expect("batch");
    assert_eq!(summary.ok, 2);
    let calls = calls.into_inner().expect("calls");
    assert_eq!(calls.len(), 4);
    assert!(calls[1] - calls[0] >= base);
    assert!(calls[2] - calls[1] >= base * 2);
    assert!(
        calls[3] - calls[2] >= base * 2,
        "new item also observes shared slowdown"
    );
}

#[test]
fn exhausted_retry_budget_fails_only_that_item() {
    let calls = AtomicUsize::new(0);
    let summary = run_with_gate(
        items(2),
        &template(),
        1,
        0,
        &mut Vec::new(),
        &|request| {
            calls.fetch_add(1, Ordering::SeqCst);
            if request.state == ClassifyState::Text("0".into()) {
                Err((ClassifyError::new(ErrorKind::Api, "HTTP 529"), true))
            } else {
                answer()
            }
        },
        Gate::new(Duration::from_micros(1)),
    )
    .expect("batch");
    assert_eq!(calls.load(Ordering::SeqCst), MAX_ATTEMPTS + 1);
    assert_eq!((summary.ok, summary.failed, summary.exit_code()), (1, 1, 6));
}

#[test]
fn concurrent_throttles_apply_one_shared_cooldown_to_all_workers() {
    let barrier = Barrier::new(2);
    let calls = Mutex::new(Vec::new());
    let base = Duration::from_millis(10);
    let summary = run_with_gate(
        items(3),
        &template(),
        2,
        0,
        &mut Vec::new(),
        &|_| {
            let index = {
                let mut calls = calls.lock().expect("calls");
                calls.push(Instant::now());
                calls.len()
            };
            if index <= 2 {
                barrier.wait();
                Err((ClassifyError::new(ErrorKind::Api, "HTTP throttled"), true))
            } else {
                answer()
            }
        },
        Gate::new(base),
    )
    .expect("batch");
    assert_eq!(summary.ok, 3);
    let calls = calls.into_inner().expect("calls");
    assert_eq!(calls.len(), 5);
    assert!(calls[2] - calls[1] >= base);
    for pair in calls[2..].windows(2) {
        assert!(
            pair[1] - pair[0] >= base * 2,
            "retries and new items share accumulated pacing"
        );
    }
}

#[test]
fn invalid_configuration_sends_nothing_even_for_empty_batches() {
    let classify = |_: &ClassifyRequest| -> Attempt { panic!("must not send") };
    for jobs in [0, 17] {
        assert!(run(items(1), &template(), jobs, 0, &mut Vec::new(), &classify).is_err());
    }
    let mut request = template();
    request.questions.clear();
    assert_eq!(
        run(Vec::new(), &request, 4, 0, &mut Vec::new(), &classify)
            .expect_err("questions")
            .exit_code(),
        4
    );
    let summary = run(Vec::new(), &template(), 4, 3, &mut Vec::new(), &classify).expect("empty");
    assert_eq!((summary.total, summary.skipped, summary.exit_code()), (3, 3, 0));
}

struct FailingWriter;
impl Write for FailingWriter {
    fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
        Err(std::io::Error::other("disk full"))
    }
    fn flush(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}

#[test]
fn output_failure_returns_an_error_without_blocking_workers() {
    let error = run(items(50), &template(), 4, 0, &mut FailingWriter, &|_| answer()).expect_err("write");
    assert_eq!(error.exit_code(), 4);
}

struct StreamingWriter(Arc<AtomicUsize>);
impl Write for StreamingWriter {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        Ok(bytes.len())
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn completion_is_flushed_while_other_items_are_running() {
    let flushed = Arc::new(AtomicUsize::new(0));
    let barrier = Barrier::new(2);
    run(
        items(2),
        &template(),
        2,
        0,
        &mut StreamingWriter(flushed.clone()),
        &|request| {
            barrier.wait();
            if request.state == ClassifyState::Text("0".into()) {
                let deadline = Instant::now() + Duration::from_secs(2);
                while flushed.load(Ordering::SeqCst) == 0 {
                    assert!(Instant::now() < deadline, "completed sibling was not flushed");
                    std::thread::yield_now();
                }
            }
            answer()
        },
    )
    .expect("batch");
    assert_eq!(flushed.load(Ordering::SeqCst), 2);
}
