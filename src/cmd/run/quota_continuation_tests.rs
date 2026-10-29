// Quota continuation must not dispatch an otherwise launchable advice peer.
// The store child guard makes an accidental continuation fail rather than invoking an agent.
use super::*;
use crate::{agent::DetectAgentsGuard, support::paths::AidHomeGuard, types::*};

#[tokio::test]
async fn caller_owned_gates_quota_continuation() {
    let dir = tempfile::tempdir().expect("home");
    let _home = AidHomeGuard::set(dir.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(dir.path());
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid, AgentKind::Oz]);
    crate::routing::scores::test_support::seed_live();
    crate::config::agent::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::cost::test_support::task(AgentKind::Codex, 0, None);
    task.id = TaskId("t-owned-quota".into());
    let policy = crate::store::execution_protocol::ExecutionPolicy {
        name: "caller-owned".into(),
        version: 1,
        agent: "codex".into(),
        model: "exact".into(),
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    store
        .insert_task_with_policy(&task, DispatchOrigin::Automatic, Some(&policy))
        .expect("task");
    let args = RunArgs {
        agent_name: "codex".into(),
        prompt: "Refactor parser".into(),
        kind: Some(crate::agent::classifier::TaskCategory::Refactoring),
        declared_difficulty: Some(TaskDifficulty::Complex),
        declared_budget: Some(TaskBudget::Premium),
        declared_urgency: Some(TaskUrgency::Background),
        declared_rigor: Some(TaskRigor::Standard),
        ..Default::default()
    };
    assert!(
        advice_route::automatic_candidate(Some(&store), &args).is_some(),
        "live peer required"
    );
    assert!(
        continue_quota(&store, &args, &task)
            .await
            .expect("policy gate")
            .is_none()
    );
    assert!(
        store
            .get_direct_retries(task.id.as_str())
            .expect("children")
            .is_empty()
    );
}
