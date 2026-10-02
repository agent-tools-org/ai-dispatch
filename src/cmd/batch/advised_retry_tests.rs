// Stored batch retries preserve complete advice routes and saved profile authority.
// Covers failed-task fallback, explicit list consumption and legacy profile recovery.
// Deps: existing task fixture, RunArgs persistence, advice and resolver.
use super::super::batch_dispatch_support::auto_fallback_args;
use super::super::batch_retry::retry_task_to_run_args;
use super::shared::make_stored_task;
use crate::agent::{DetectAgentsGuard, classifier::TaskCategory};
use crate::cmd::run::{RunArgs, advice_route};
use crate::{store::Store, types::*};

fn args(dir: &std::path::Path) -> RunArgs {
    RunArgs {
        agent_name: "oz".into(),
        prompt: "Compare documentation".into(),
        dir: Some(dir.display().to_string()),
        kind: Some(TaskCategory::Refactoring),
        declared_difficulty: Some(TaskDifficulty::Complex),
        declared_budget: Some(TaskBudget::Premium),
        declared_urgency: Some(TaskUrgency::Background),
        declared_rigor: Some(TaskRigor::Standard),
        team: Some("routes".into()),
        ..Default::default()
    }
}

fn setup() -> (
    tempfile::TempDir,
    crate::paths::AidHomeGuard,
    crate::live_quota::CacheDirGuard,
    Store,
) {
    let dir = tempfile::tempdir().expect("home");
    let home = crate::paths::AidHomeGuard::set(dir.path());
    let cache = crate::live_quota::CacheDirGuard::set(dir.path());
    crate::agent::codex::cli_config::set_test_codex_home(Some(dir.path().join("no-codex")));
    std::fs::create_dir(crate::team::teams_dir()).expect("teams");
    std::fs::write(
        crate::team::teams_dir().join("routes.toml"),
        "[team]\nid = 'routes'\ndisplay_name = 'Routes'\npreferred_agents = ['droid', 'claude']\n",
    )
    .expect("team");
    (dir, home, cache, Store::open_memory().expect("store"))
}

#[test]
fn held_batch_retry_uses_saved_profile_not_row_category_and_keeps_candidate_model() {
    let (dir, _home, _cache, store) = setup();
    let _fleet =
        DetectAgentsGuard::set(vec![AgentKind::Droid, AgentKind::Claude, AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    let mut task = make_stored_task("t-batch-advice", AgentKind::Oz, TaskStatus::Failed);
    task.category = Some("research".into());
    task.agent_session_id = Some("old-session".into());
    store.insert_task(&task).expect("task");
    let mut saved = args(dir.path());
    saved.session_id = task.agent_session_id.clone();
    store
        .update_task_dispatch_args(
            task.id.as_str(),
            &saved.dispatch_args_json().expect("serialize"),
        )
        .expect("save");
    store
        .update_task_profile(
            task.id.as_str(),
            TaskProfileDeclaration {
                difficulty: Some(TaskDifficulty::Trivial),
                budget: Some(TaskBudget::Cheap),
                ..Default::default()
            },
        )
        .expect("row profile");
    std::fs::write(
        dir.path().join("rate-limit-oz"),
        "hold: manual\nmessage: quota exhausted\n",
    )
    .expect("hold");
    saved.prompt = task.prompt.clone();
    let candidate = advice_route::automatic_candidate(Some(&store), &saved).expect("candidate");
    let retry = retry_task_to_run_args(&store, &task, "wg-test", None).expect("retry");
    assert_eq!(retry.agent_name, candidate.agent);
    assert_eq!(retry.model, candidate.model);
    assert_eq!(retry.kind, saved.kind);
    assert_eq!(retry.team, saved.team);
    assert_eq!(
        advice_route::report(Some(&store), &retry).declared,
        advice_route::report(Some(&store), &saved).declared
    );
    assert!(retry.session_id.is_none() && retry.model_source == crate::agent::model_validation::ModelSource::Advised);
    assert_eq!(retry.parent_task_id.as_deref(), Some(task.id.as_str()));
}

#[test]
fn failed_batch_auto_fallback_returns_exact_candidate_args_from_saved_dispatch() {
    let (dir, _home, _cache, store) = setup();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("codex", Some("gpt-6-sol")).expect("model");
    let task = make_stored_task("t-failed-advice", AgentKind::Oz, TaskStatus::Failed);
    store.insert_task(&task).expect("task");
    let saved = args(dir.path());
    store
        .update_task_dispatch_args(
            task.id.as_str(),
            &saved.dispatch_args_json().expect("serialize"),
        )
        .expect("save");
    let mut input = saved.clone();
    input.prompt = task.prompt.clone();
    let candidate = advice_route::automatic_candidate(Some(&store), &input).expect("candidate");
    let spec = toml::from_str("agent = 'oz'\nprompt = 'different'").expect("spec");
    let (original, fallback) = auto_fallback_args(&store, task.id.as_str(), &[spec], 0)
        .expect("fallback")
        .expect("route");
    assert_eq!(original, "oz");
    assert_eq!(fallback.agent_name, candidate.agent);
    assert_eq!(fallback.model, candidate.model);
    assert_eq!(fallback.model_source, crate::agent::model_validation::ModelSource::Advised);
    assert_eq!(fallback.declared_difficulty, saved.declared_difficulty);
    assert_eq!(fallback.declared_budget, saved.declared_budget);
    assert_eq!(fallback.declared_urgency, saved.declared_urgency);
    assert_eq!(fallback.declared_rigor, saved.declared_rigor);
}

#[test]
fn saved_explicit_batch_list_keeps_order_custom_resolution_and_remaining_entries() {
    let (dir, _home, _cache, store) = setup();
    std::fs::create_dir(dir.path().join("agents")).expect("agents");
    std::fs::write(
        dir.path().join("agents/custom-peer.toml"),
        "[agent]\nid = 'custom-peer'\ndisplay_name = 'Custom'\ncommand = 'custom-peer'\n",
    )
    .expect("custom");
    let task = make_stored_task("t-explicit-batch", AgentKind::Oz, TaskStatus::Failed);
    store.insert_task(&task).expect("task");
    let mut saved = args(dir.path());
    saved.cascade = vec![
        "oz".into(),
        "custom-peer".into(),
        "gemini".into(),
        "claude".into(),
    ];
    store
        .update_task_dispatch_args(
            task.id.as_str(),
            &saved.dispatch_args_json().expect("serialize"),
        )
        .expect("save");
    let (_, fallback) = auto_fallback_args(&store, task.id.as_str(), &[], 0)
        .expect("fallback")
        .expect("custom");
    assert_eq!(fallback.agent_name, "custom-peer");
    assert_eq!(fallback.cascade, ["gemini", "claude"]);
    assert_ne!(fallback.model_source, crate::agent::model_validation::ModelSource::Advised);
    saved.cascade.push("unknown-peer".into());
    store
        .update_task_dispatch_args(
            task.id.as_str(),
            &saved.dispatch_args_json().expect("serialize"),
        )
        .expect("save");
    assert!(
        auto_fallback_args(&store, task.id.as_str(), &[], 0)
            .err()
            .expect("unknown")
            .to_string()
            .contains("unknown-peer")
    );
}

#[test]
fn legacy_retry_recovers_stored_category_and_profile_only_without_saved_args() {
    let (dir, _home, _cache, store) = setup();
    let mut task = make_stored_task("t-legacy-profile", AgentKind::Oz, TaskStatus::Failed);
    task.category = Some("refactoring".into());
    store.insert_task(&task).expect("task");
    let profile = TaskProfileDeclaration {
        difficulty: Some(TaskDifficulty::Complex),
        budget: Some(TaskBudget::Premium),
        urgency: Some(TaskUrgency::Urgent),
        rigor: Some(TaskRigor::Standard),
    };
    store
        .update_task_profile(task.id.as_str(), profile)
        .expect("profile");
    let legacy = RunArgs::for_retry(&store, &task).expect("legacy");
    assert_eq!(legacy.kind, Some(TaskCategory::Refactoring));
    assert_eq!(legacy.declared_difficulty, profile.difficulty);
    assert_eq!(legacy.declared_budget, profile.budget);
    assert_eq!(legacy.declared_urgency, profile.urgency);
    assert_eq!(legacy.declared_rigor, profile.rigor);
    let saved = RunArgs {
        dir: Some(dir.path().display().to_string()),
        ..Default::default()
    };
    store
        .update_task_dispatch_args(
            task.id.as_str(),
            &saved.dispatch_args_json().expect("serialize"),
        )
        .expect("save");
    let restored = RunArgs::for_retry(&store, &task).expect("saved");
    assert!(restored.kind.is_none() && restored.declared_difficulty.is_none());
    assert!(
        restored.declared_budget.is_none()
            && restored.declared_urgency.is_none()
            && restored.declared_rigor.is_none()
    );
}

#[test]
fn saved_empty_cascade_preserves_explicit_exhaustion_even_with_conflicting_spec() {
    let (dir, _home, _cache, store) = setup();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Claude]);
    let task = make_stored_task("t-exhausted-batch", AgentKind::Oz, TaskStatus::Failed);
    store.insert_task(&task).expect("task");
    let mut saved = args(dir.path());
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().expect("serialize")).expect("save");
    assert!(advice_route::automatic_candidate(Some(&store), &saved).is_some());
    for fallback in ["oz", "claude", "unknown-peer", ""] {
        let spec = toml::from_str(&format!("agent = 'oz'\nprompt = 'different'\nfallback = '{fallback}'")).expect("spec");
        assert!(auto_fallback_args(&store, task.id.as_str(), &[spec], 0).expect("exhausted").is_none());
    }
    // Remaining saved entries are authoritative over a conflicting current specification.
    saved.cascade = vec!["gemini".into(), "claude".into()];
    store.update_task_dispatch_args(task.id.as_str(), &saved.dispatch_args_json().expect("serialize")).expect("save");
    let spec = toml::from_str("agent = 'oz'\nprompt = 'different'\nfallback = 'unknown-peer'").expect("spec");
    let (_, next) = auto_fallback_args(&store, task.id.as_str(), &[spec], 0).expect("saved list").expect("Gemini");
    assert_eq!(next.agent_name, "gemini");
    assert_eq!(next.cascade, ["claude"]);
}
