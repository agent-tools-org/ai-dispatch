// Tests for dispatch preparation result-file defaults.
// Exports: none.
// Deps: super::prepare_dispatch, crate::store, RunArgs.

use super::*;
use std::sync::Arc;

#[test]
fn prepare_dispatch_uses_task_specific_audit_result_file() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Review the implementation and list findings.".to_string(),
        read_only: true,
        ..Default::default()
    };

    let prepared = prepare_dispatch(&store, &mut args).unwrap();

    assert_eq!(
        args.result_file.as_deref(),
        Some(crate::cmd::report_mode::task_result_file(prepared.task_id.as_str()).as_str())
    );
}

#[test]
fn prepare_dispatch_skips_auto_result_file_when_output_is_set() {
    let store = Arc::new(Store::open_memory().unwrap());
    let mut args = RunArgs {
        agent_name: "codex".to_string(),
        prompt: "Review the implementation and list findings.".to_string(),
        read_only: true,
        output: Some("audit.md".to_string()),
        ..Default::default()
    };

    prepare_dispatch(&store, &mut args).unwrap();

    assert_eq!(args.result_file, None);
}
