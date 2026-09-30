// Batch classification: validated input, resumable output, and shared scheduling.
// Exports: input/output helpers, run, Summary; depends on the existing classify path.

mod io;
mod runner;
mod throttle;

pub(crate) use io::{open_output, read_items, successful_ids};
pub(crate) use runner::run;

use super::classify::{ClassifyError, ClassifyState, ErrorKind};
use super::question::Declared;
use serde_json::Value;
use std::collections::BTreeMap;

pub(crate) const PARTIAL_FAILURE_EXIT: i32 = 6;

#[derive(Debug)]
pub(crate) struct Item {
    pub id: String,
    pub state: ClassifyState,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Summary {
    pub total: usize,
    pub ok: usize,
    pub failed: usize,
    pub skipped: usize,
    pub choices: BTreeMap<String, BTreeMap<String, usize>>,
}

impl Summary {
    pub(crate) fn new(total: usize, skipped: usize, declared: &BTreeMap<String, Declared>) -> Self {
        let choices = declared
            .iter()
            .filter_map(|(id, question)| {
                let Declared::Choice(options) = question else {
                    return None;
                };
                Some((id.clone(), options.iter().map(|option| (option.clone(), 0)).collect()))
            })
            .collect();
        Self {
            total,
            ok: 0,
            failed: 0,
            skipped,
            choices,
        }
    }

    fn record(&mut self, result: &Result<Value, ClassifyError>) {
        match result {
            Ok(value) => {
                self.ok += 1;
                for (id, options) in &mut self.choices {
                    if let Some(count) = value["answers"][id]["choice"]
                        .as_str()
                        .and_then(|choice| options.get_mut(choice))
                    {
                        *count += 1;
                    }
                }
            }
            Err(_) => self.failed += 1,
        }
    }

    pub(crate) fn exit_code(&self) -> i32 {
        if self.failed == 0 { 0 } else { PARTIAL_FAILURE_EXIT }
    }
}

impl std::fmt::Display for Summary {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(
            f,
            "total={} ok={} failed={} skipped={}",
            self.total, self.ok, self.failed, self.skipped
        )?;
        for (id, options) in &self.choices {
            writeln!(f, "choice {id}: {}", serde_json::json!(options))?;
        }
        Ok(())
    }
}

fn invalid(message: impl Into<String>) -> ClassifyError {
    ClassifyError::new(ErrorKind::Invalid, message)
}

#[cfg(test)]
mod tests;
