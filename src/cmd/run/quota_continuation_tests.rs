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
        network: false,
        accepted_at: "2026-10-29T10:00:00Z".into(),
    };
    store
        .insert_task_with_policy(&task, DispatchOrigin::Automatic, Some(&policy))
        .expect("task");
    let args = RunArgs {
        policy: Some("caller-owned".into()),
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

#[tokio::test]
async fn agy_family_quota_continuation_stays_on_same_agent() {
    let home = crate::support::test_env::TestHome::new();
    let cache = tempfile::tempdir().expect("cache");
    let _cache = crate::routing::quota::live::CacheDirGuard::set(cache.path());
    let _served = crate::agent::model_validation::MockServedModelsGuard::set(
        AgentKind::Antigravity,
        Some(vec![
            "gemini-4-argon-high".into(),
            "claude-opus-5-5-high".into(),
        ]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut task = crate::cost::test_support::task(AgentKind::Antigravity, 0, None);
    task.status = crate::types::TaskStatus::Failed;
    task.requested_model = Some("gemini-4-argon-high".into());
    store.insert_task(&task).expect("parent");
    crate::routing::rate_limit::mark_untrusted_for_model(
        &task.agent,
        None,
        task.requested_model.as_deref(),
        "Individual quota reached. Resets in 36h29m20s.",
    );
    let args = RunArgs {
        agent_name: "agy".into(),
        prompt: "say hi".into(),
        dry_run: true,
        dir: Some(home.path().display().to_string()),
        ..Default::default()
    };
    let id = continue_quota(&store, &args, &task)
        .await
        .expect("continuation")
        .expect("same agent child");
    let child = store.get_task(id.as_str()).expect("get").expect("child");
    assert_eq!(child.agent, AgentKind::Antigravity);
    assert_eq!(
        child.requested_model.as_deref(),
        Some("claude-opus-5-5-high")
    );
}
