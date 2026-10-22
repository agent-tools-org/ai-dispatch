// Tests for batch command module behavior.
// Exports: (tests only)
// Deps: nested test modules
#[path = "batch_tests/helpers.rs"]
mod helpers;
#[path = "batch_tests/retry.rs"]
mod retry;
#[path = "batch_tests/run_args.rs"]
mod run_args;
#[path = "batch_tests/shared.rs"]
mod shared;
#[path = "batch_tests/trigger.rs"]
mod trigger;

#[path = "advised_retry_tests.rs"]
mod advised_retry;
#[path = "fallback_scope_tests.rs"]
mod fallback_scope;

#[path = "batch_tests/helpers_completion.rs"]
mod helpers_completion;
#[path = "batch_tests/retry_execution.rs"]
mod retry_execution;
