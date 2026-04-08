// Tests for best-of dispatch ID derivation and winner selection.
// Keeps run_bestof.rs focused on runtime flow while preserving regressions.

use super::*;
use crate::store::Store;

#[test]
fn pick_best_result_prefers_longest_diff() {
    let winner = CandidateResult {
        task_id: TaskId::generate(),
        agent_label: "kilo".to_string(),
        status: TaskStatus::Done,
        diff_line_count: 12,
        metric_score: None,
    };
    let runner = CandidateResult {
        task_id: TaskId::generate(),
        agent_label: "cursor".to_string(),
        status: TaskStatus::Done,
        diff_line_count: 3,
        metric_score: None,
    };
    let failed = CandidateResult {
        task_id: TaskId::generate(),
        agent_label: "gemini".to_string(),
        status: TaskStatus::Failed,
        diff_line_count: 0,
        metric_score: None,
    };
    let results = vec![winner.clone(), runner, failed];
    let best = pick_best_result(&results).unwrap();
    assert_eq!(best.task_id, winner.task_id);
}

#[test]
fn pick_best_result_none_when_no_done() {
    let failed = CandidateResult {
        task_id: TaskId::generate(),
        agent_label: "opencode".to_string(),
        status: TaskStatus::Failed,
        diff_line_count: 0,
        metric_score: None,
    };
    assert!(pick_best_result(&[failed]).is_none());
}

#[test]
fn pick_best_result_prefers_metric_score() {
    let candidates = vec![
        CandidateResult {
            task_id: TaskId("t-1".into()),
            agent_label: "a".into(),
            status: TaskStatus::Done,
            diff_line_count: 100,
            metric_score: Some(3.0),
        },
        CandidateResult {
            task_id: TaskId("t-2".into()),
            agent_label: "b".into(),
            status: TaskStatus::Done,
            diff_line_count: 10,
            metric_score: Some(9.0),
        },
    ];
    let best = pick_best_result(&candidates).unwrap();
    assert_eq!(best.task_id, TaskId("t-2".into()));
}

#[test]
fn pick_best_result_treats_merged_as_success() {
    let merged = CandidateResult {
        task_id: TaskId("t-merged".into()),
        agent_label: "a".into(),
        status: TaskStatus::Merged,
        diff_line_count: 4,
        metric_score: None,
    };
    let failed = CandidateResult {
        task_id: TaskId("t-failed".into()),
        agent_label: "b".into(),
        status: TaskStatus::Failed,
        diff_line_count: 10,
        metric_score: None,
    };
    let candidates = [failed, merged.clone()];
    let best = pick_best_result(&candidates).unwrap();
    assert_eq!(best.task_id, merged.task_id);
}

#[test]
fn pick_best_result_ignores_nan_metric_scores() {
    let candidates = vec![
        CandidateResult {
            task_id: TaskId("t-nan".into()),
            agent_label: "a".into(),
            status: TaskStatus::Done,
            diff_line_count: 100,
            metric_score: Some(f64::NAN),
        },
        CandidateResult {
            task_id: TaskId("t-finite".into()),
            agent_label: "b".into(),
            status: TaskStatus::Done,
            diff_line_count: 1,
            metric_score: Some(9.0),
        },
    ];
    let best = pick_best_result(&candidates).unwrap();
    assert_eq!(best.task_id, TaskId("t-finite".into()));
}

#[test]
fn best_of_count_validation() {
    assert!(validate_best_of_count(2).is_ok());
    assert!(validate_best_of_count(5).is_ok());
    assert!(validate_best_of_count(1).is_err());
    assert!(validate_best_of_count(0).is_err());
}

#[test]
fn best_of_completion_includes_awaiting_input() {
    assert!(is_completed_best_of_status(&TaskStatus::AwaitingInput));
    assert!(is_completed_best_of_status(&TaskStatus::Done));
    assert!(!is_completed_best_of_status(&TaskStatus::Running));
}

#[test]
fn best_of_task_ids_reuse_base_while_it_is_unclaimed() {
    let base = TaskId("t-batch".into());
    let store = Store::open_memory().unwrap();
    let first = best_of_task_id(&store, Some(&base), 0).unwrap();
    let third = best_of_task_id(&store, Some(&base), 2).unwrap();
    assert_eq!(first, Some(TaskId("t-batch".into())));
    assert_eq!(third, Some(TaskId("t-batch".into())));
}

#[test]
fn best_of_task_ids_truncate_to_fit_task_limit() {
    let store = Store::open_memory().unwrap();
    let base = TaskId(format!("t-{}", "a".repeat(62)));
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    let derived = best_of_task_id(&store, Some(&base), 4).unwrap().unwrap();
    assert_eq!(derived.as_str().len(), 64);
    assert!(derived.as_str().ends_with("-bo5"));
}

#[test]
fn best_of_task_ids_reuse_waiting_placeholder_until_claimed() {
    let store = Store::open_memory().unwrap();
    let base = TaskId("t-batch".into());
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    let second = best_of_task_id(&store, Some(&base), 1).unwrap();
    assert_eq!(second, Some(base.clone()));
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    let after_claim = best_of_task_id(&store, Some(&base), 1).unwrap();
    assert_eq!(after_claim, Some(TaskId("t-batch-bo2".into())));
}

#[test]
fn best_of_task_ids_fall_back_to_random_when_derived_id_is_running() {
    let store = Store::open_memory().unwrap();
    let base = TaskId("t-batch".into());
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    store
        .insert_waiting_task(
            "t-batch-bo2",
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status("t-batch-bo2", TaskStatus::Running)
        .unwrap();
    let candidate = best_of_task_id(&store, Some(&base), 1).unwrap();
    assert_eq!(candidate, None);
}

#[test]
fn best_of_task_ids_drop_invalid_auto_suffixes() {
    let store = Store::open_memory().unwrap();
    let base = TaskId(format!("t-{}", "a".repeat(62)));
    let derived = format!("t-{}-bo5", "a".repeat(58));
    store
        .insert_waiting_task(
            base.as_str(),
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(base.as_str(), TaskStatus::Pending)
        .unwrap();
    store
        .insert_waiting_task(
            &derived,
            "codex",
            "prompt",
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    store
        .update_task_status(&derived, TaskStatus::Pending)
        .unwrap();
    let candidate = best_of_task_id(&store, Some(&base), 4).unwrap();
    assert_eq!(candidate, None);
}

#[test]
fn best_of_task_ids_reject_invalid_base_ids_before_reuse() {
    let store = Store::open_memory().unwrap();
    let err = best_of_task_id(&store, Some(&TaskId("-bad".into())), 0).unwrap_err();
    assert!(err
        .to_string()
        .contains("Invalid task ID"));
}
