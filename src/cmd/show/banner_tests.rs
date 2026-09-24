// Tests for the `aid show` missing-result banner: it needs a recorded result-file request.
// Exports: none.
// Deps: super show helpers, tests::task_fixture, RunArgs, Store.

use super::tests::task_fixture;
use super::*;
use crate::cmd::run::RunArgs;
use crate::types::TaskStatus;
use std::sync::Arc;

fn request_result_file(store: &Store, task_id: &str) {
    let args = RunArgs { result_file: Some(format!("result-{task_id}.md")), ..Default::default() };
    store.update_task_dispatch_args(task_id, &args.dispatch_args_json().unwrap()).unwrap();
}

#[test]
fn writable_task_with_audit_words_and_no_result_request_has_no_banner() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture(
        "t-writable",
        "Build the eval for cross-audit detection; report findings with severity.",
        None,
        None,
    );
    task.status = TaskStatus::Done;
    store.insert_task(&task).unwrap();
    let args = RunArgs { kind: Some(crate::agent::classifier::TaskCategory::ComplexImpl), ..Default::default() };
    store.update_task_dispatch_args("t-writable", &args.dispatch_args_json().unwrap()).unwrap();

    let text = audit_text(&store, "t-writable").unwrap();

    assert!(!text.contains("Structured audit result missing"));
}

#[test]
fn audit_text_with_missing_result_md_shows_banner() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture(
        "t-audit-missing",
        "Cross-audit the parser changes and produce findings.",
        None,
        None,
    );
    task.status = TaskStatus::Done;
    task.read_only = true;
    store.insert_task(&task).unwrap();
    request_result_file(&store, "t-audit-missing");

    let text = audit_text(&store, "t-audit-missing").unwrap();

    assert!(text.contains("Structured audit result missing"));
    assert!(text.contains("aid retry t-audit-missing --agent codex"));
}

#[test]
fn result_text_for_audit_without_result_md_uses_banner() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture(
        "t-audit-result-missing",
        "Review the implementation and list findings with severity.",
        None,
        None,
    );
    task.status = TaskStatus::Done;
    task.read_only = true;
    store.insert_task(&task).unwrap();
    request_result_file(&store, "t-audit-result-missing");

    let text = result_text(&store, "t-audit-result-missing").unwrap();

    assert!(text.contains("Structured audit result missing"));
}

/// A result.md salvaged from the log must not silence the banner: that file holds the
/// tool narration aid rescued, not the audit the caller asked for.
#[test]
fn result_text_for_audit_warns_when_result_md_is_a_salvaged_log() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let store = Arc::new(Store::open_memory().unwrap());
    let mut task = task_fixture(
        "t-audit-salvaged",
        "Review the implementation and list findings with severity.",
        None,
        None,
    );
    task.status = TaskStatus::Done;
    task.read_only = true;
    store.insert_task(&task).unwrap();
    request_result_file(&store, "t-audit-salvaged");
    store
        .update_delivery_assessment(
            "t-audit-salvaged",
            Some(crate::types::DeliveryAssessment::MissingFinalDelivery),
        )
        .unwrap();
    let result_path = crate::paths::task_dir("t-audit-salvaged").join("result.md");
    std::fs::create_dir_all(result_path.parent().unwrap()).unwrap();
    std::fs::write(&result_path, "I will run `git diff main..HEAD` to see the changes.\n").unwrap();

    let text = result_text(&store, "t-audit-salvaged").unwrap();

    assert!(text.contains("Structured audit result missing"));
}
