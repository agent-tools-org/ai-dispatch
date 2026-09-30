// Scoped std workers classify independently and stream completions to one writer.
// Exports: run; dependencies: shared classify preparation, throttle gate, and JSONL I/O.

use super::{Item, Summary, invalid, throttle::Gate};
use crate::typesafe::classify::{Attempt, ClassifyError, ClassifyRequest, ErrorKind, prepare};
use serde_json::{Value, json};
use std::io::Write;
use std::sync::{Mutex, mpsc};
use std::time::{Duration, Instant};

const MAX_ATTEMPTS: usize = 5;

pub(crate) fn run(
    items: Vec<Item>,
    template: &ClassifyRequest,
    jobs: usize,
    skipped: usize,
    output: &mut impl Write,
    classifier: &(impl Fn(&ClassifyRequest) -> Attempt + Sync),
) -> Result<Summary, ClassifyError> {
    run_with_gate(
        items,
        template,
        jobs,
        skipped,
        output,
        classifier,
        Gate::new(Duration::from_secs(1)),
    )
}

fn run_with_gate(
    items: Vec<Item>,
    template: &ClassifyRequest,
    jobs: usize,
    skipped: usize,
    output: &mut impl Write,
    classifier: &(impl Fn(&ClassifyRequest) -> Attempt + Sync),
    gate: Gate,
) -> Result<Summary, ClassifyError> {
    if !(1..=16).contains(&jobs) {
        return Err(invalid("--jobs must be 1-16"));
    }
    let mut summary = Summary::new(items.len() + skipped, skipped, &prepare(template)?);
    let workers = jobs.min(items.len());
    let queue = Mutex::new(items.into_iter());
    let (tx, rx) = mpsc::sync_channel(workers.max(1));
    std::thread::scope(|scope| {
        for _ in 0..workers {
            let (tx, queue, gate) = (tx.clone(), &queue, &gate);
            scope.spawn(move || {
                loop {
                    let next = queue.lock().unwrap_or_else(|error| error.into_inner()).next();
                    let Some(item) = next else { break };
                    let mut request = template.clone();
                    request.state = item.state;
                    let result = classify_item(&request, gate, classifier);
                    if tx.send((item.id, result)).is_err() {
                        break;
                    }
                }
            });
        }
        drop(tx);
        for (id, result) in rx {
            let row = output_row(&id, &result);
            writeln!(output, "{row}")
                .and_then(|()| output.flush())
                .map_err(|_| invalid("cannot append batch result to --out"))?;
            summary.record(&result);
        }
        Ok(summary)
    })
}

fn classify_item(
    request: &ClassifyRequest,
    gate: &Gate,
    classifier: &(impl Fn(&ClassifyRequest) -> Attempt + Sync),
) -> Result<Value, ClassifyError> {
    prepare(request)?;
    let started = Instant::now();
    for attempt in 1..=MAX_ATTEMPTS {
        gate.enter();
        match classifier(request) {
            Ok(mut value) => {
                value["latency_ms"] = json!(u64::try_from(started.elapsed().as_millis()).unwrap_or(u64::MAX));
                return Ok(value);
            }
            Err((error, throttled)) => {
                if throttled {
                    gate.throttled();
                }
                if !throttled || attempt == MAX_ATTEMPTS {
                    return Err(error);
                }
            }
        }
    }
    Err(ClassifyError::new(ErrorKind::Api, "batch retry budget exhausted"))
}

fn output_row(id: &str, result: &Result<Value, ClassifyError>) -> Value {
    match result {
        Ok(value) => json!({
            "id": id, "ok": true, "answers": value["answers"], "model": value["model"],
            "usage": value["usage"], "latency_ms": value["latency_ms"],
        }),
        Err(error) => {
            let kind = match error.kind {
                ErrorKind::NoKey => "no_key",
                ErrorKind::Api => "api",
                ErrorKind::Invalid => "invalid",
                ErrorKind::Refused => "refused",
            };
            json!({"id": id, "ok": false, "error_kind": kind, "error": error.message})
        }
    }
}

#[cfg(test)]
#[path = "runner_tests.rs"]
mod tests;
