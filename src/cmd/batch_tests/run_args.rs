// Tests for task_to_run_args conversion details.
// Exports: (tests only)
// Deps: crate::batch, batch_args
use crate::batch;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use std::sync::Arc;

use super::super::batch_args::task_to_run_args;

fn isolated_home() -> AidHomeGuard {
    let temp = tempfile::tempdir().unwrap();
    AidHomeGuard::set(temp.path())
}

#[path = "run_args_cases_1.rs"]
mod run_args_cases_1;
#[path = "run_args_cases_2.rs"]
mod run_args_cases_2;
#[path = "run_args_cases_3.rs"]
mod run_args_cases_3;
