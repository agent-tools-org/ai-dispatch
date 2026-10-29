// Lifecycle gates use the already loaded dispatch policy, even if the row later differs.
// Invalid execution data proves an accidental recovery lookup crossed the gate.
use super::*;

#[tokio::test]
async fn caller_owned_lifecycle_uses_saved_policy_without_reading_policy_rows_again() {
    let (store, id, args) = setup();
    store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy=NULL WHERE id=?1",
            [id.as_str()],
        )
        .expect("the policy was already loaded");
    assert!(
        run_model_selfheal::maybe_auto_retry_after_model_unavailable(&store, &id, &args)
            .await
            .expect("saved policy")
            .is_none()
    );
    assert!(
        run_post::maybe_auto_retry_after_hang(&store, &id, &args)
            .await
            .expect("saved policy")
            .is_none()
    );
    assert!(
        run_delivery_recovery::maybe_auto_recover_missing_delivery(&store, &id, &args)
            .await
            .expect("saved policy")
            .is_none()
    );
    assert!(
        maybe_auto_retry_after_verify_failure(&store, &id, &args, TaskStatus::Done)
            .await
            .expect("saved policy")
            .is_none()
    );
}

#[tokio::test]
async fn caller_owned_completion_effects_use_the_already_loaded_policy() {
    let (store, id, args) = setup();
    store
        .db()
        .execute(
            "UPDATE tasks SET execution_policy=NULL WHERE id=?1",
            [id.as_str()],
        )
        .expect("the policy was already loaded");
    let missing = crate::cmd::checklist_scan::ChecklistResult {
        items: vec![crate::cmd::checklist_scan::ChecklistItemResult {
            item: "required".into(),
            status: crate::cmd::checklist_scan::ChecklistItemStatus::Missing,
        }],
    };
    assert!(
        maybe_auto_retry_after_checklist_miss(&store, &id, &args, Some(&missing))
            .await
            .expect("saved policy")
            .is_none()
    );
    assert!(
        maybe_judge_retry(&store, &args, &id)
            .await
            .expect("saved policy")
            .is_none()
    );
    assert!(
        run_lifecycle::retry_or_cascade(&store, &args, &id)
            .await
            .expect("saved policy")
            .is_none()
    );
    run_post::maybe_run_post_done_audit(&store, &id, &args, None, None).expect("saved policy");
}
