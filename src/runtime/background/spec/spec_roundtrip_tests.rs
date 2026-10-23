// Serialization and backwards-compatibility coverage for worker job specs.
use super::BackgroundRunSpec;
use crate::cmd::run::RunArgs;

fn sample_value(read_only: bool) -> serde_json::Value {
    let mut value: serde_json::Value =
        serde_json::from_str(include_str!("../../../../testdata/full-background-spec.json")).unwrap();
    value["read_only"] = read_only.into();
    value
}

fn sample_spec(read_only: bool) -> BackgroundRunSpec {
    serde_json::from_value(sample_value(read_only)).unwrap()
}

#[test]
fn background_run_spec_round_trips_read_only() {
    let job = serde_json::to_value(sample_spec(true)).unwrap();
    assert_eq!(job.as_object().unwrap().len(), 15);
    assert!(job.get("read_only").is_none());
    let args: RunArgs = serde_json::from_value(sample_value(true)).unwrap();
    let value = serde_json::to_value(&args).unwrap();
    assert_eq!(value.get("read_only").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(value.get("result_file").and_then(|v| v.as_str()), Some("result.md"));
    assert_eq!(value.get("result_file_required").and_then(|v| v.as_bool()), Some(true));
    assert_eq!(value.get("iterate").and_then(|v| v.as_u64()), Some(3));
    assert!(job.get("pre_task_dirty_paths").is_some());
    assert_eq!(value.get("audit").and_then(|v| v.as_bool()), Some(true));

    let decoded: RunArgs = serde_json::from_value(value).unwrap();
    assert!(decoded.read_only);
    assert!(decoded.audit_report_mode);
    let runtime: BackgroundRunSpec = serde_json::from_value(job).unwrap();
    assert!(runtime.audit_report_mode);
    assert!(runtime.budget);
    assert_eq!(decoded.result_file.as_deref(), Some("result.md"));
    assert_eq!(decoded.result_file_required, Some(true));
    assert_eq!(decoded.eval.as_deref(), Some("cargo test"));
    assert_eq!(
        runtime.pre_task_dirty_paths.as_deref(),
        Some(&["?? pre-existing.rs".to_string()][..])
    );
    assert_eq!(decoded.hooks, ["on_fail:echo fail".to_string()]);
    assert_eq!(decoded.worktree.as_deref(), Some("feature/retry"));
    assert_eq!(decoded.base_branch.as_deref(), Some("main"));
    assert_eq!(decoded.peer_review.as_deref(), Some("codex"));
    assert_eq!(decoded.scope, ["src".to_string()]);
    assert!(decoded.budget);
    assert_eq!(decoded.session_id.as_deref(), Some("session-123"));
    assert_eq!(decoded.max_task_cost, Some(2.5));
    assert!(decoded.judge_retry);
    assert_eq!(runtime.agent_started.as_deref(), Some("launch-22"));
}

#[test]
fn background_run_spec_defaults_read_only_to_false_when_missing() {
    let mut value = sample_value(false);
    value.as_object_mut().unwrap().remove("read_only");
    value.as_object_mut().unwrap().remove("audit_report_mode");
    value.as_object_mut().unwrap().remove("hooks");
    value.as_object_mut().unwrap().remove("audit");

    let runtime: BackgroundRunSpec = serde_json::from_value(value.clone()).unwrap();
    assert!(!runtime.audit_report_mode);
    let decoded: RunArgs = serde_json::from_value(value).unwrap();
    assert!(!decoded.read_only);
    assert!(!decoded.audit_report_mode);
    assert!(decoded.hooks.is_empty());
    assert!(!decoded.audit);
}

#[test]
fn legacy_detached_field_is_ignored() {
    let mut value = serde_json::to_value(sample_spec(false)).unwrap();
    value
        .as_object_mut()
        .unwrap()
        .insert("detached".to_string(), serde_json::Value::Bool(true));

    let decoded = serde_json::from_value::<BackgroundRunSpec>(value).unwrap();
    assert_eq!(decoded.task_id, "t-1234");
}
