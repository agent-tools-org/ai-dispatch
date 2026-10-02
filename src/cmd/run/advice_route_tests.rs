// Automatic cascade selection compared with the existing advice report.
// Covers exclusions, exact model/default application and both held phases.
// Deps: isolated routing inventory, Store, resolver, model and quota mocks.
use super::*;
use crate::agent::{DetectAgentsGuard, classifier::TaskCategory};
use crate::types::{TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};
use std::sync::Arc;

fn isolated() -> (
    tempfile::TempDir,
    crate::paths::AidHomeGuard,
    crate::live_quota::CacheDirGuard,
) {
    let dir = tempfile::tempdir().expect("home");
    let home = crate::paths::AidHomeGuard::set(dir.path());
    let cache = crate::live_quota::CacheDirGuard::set(dir.path());
    crate::agent::codex::cli_config::set_test_codex_home(Some(dir.path().join("no-codex")));
    (dir, home, cache)
}

fn args() -> RunArgs {
    RunArgs {
        agent_name: "oz".into(),
        prompt: "Compare documentation".into(),
        kind: Some(TaskCategory::Refactoring),
        declared_difficulty: Some(TaskDifficulty::Complex),
        declared_budget: Some(TaskBudget::Premium),
        declared_urgency: Some(TaskUrgency::Background),
        declared_rigor: Some(TaskRigor::Standard),
        ..Default::default()
    }
}

fn hold(dir: &std::path::Path, route: &str) {
    std::fs::write(
        dir.join(format!("rate-limit-{route}")),
        "hold: manual\nmessage: quota exhausted\n",
    )
    .expect("hold");
}

fn expected(store: &Store, args: &RunArgs) -> Option<AdviceCandidate> {
    let declared = DeclaredTaskProfile {
        difficulty: args.declared_difficulty.unwrap_or_default(),
        budget: args.declared_budget.unwrap_or_default(),
        urgency: args.declared_urgency.unwrap_or_default(),
        rigor: args.declared_rigor.unwrap_or_default(),
    };
    crate::cmd::advise::build_report(
        Some(store),
        &args.prompt,
        declared,
        args.kind,
        args.team.as_deref(),
        0,
        None,
    )
    .candidates
    .into_iter()
    .find(|c| c.agent != args.agent_name && c.agent != "claude" && c.launchable(None))
}

#[test]
fn automatic_uses_first_report_route_with_complete_profile_kind_team_and_store() {
    let (_dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![
        AgentKind::Codex,
        AgentKind::Droid,
        AgentKind::Claude,
        AgentKind::Oz,
    ]);
    std::fs::create_dir(crate::team::teams_dir()).expect("teams");
    std::fs::write(crate::team::teams_dir().join("routes.toml"),
        "[team]\nid = 'routes'\ndisplay_name = 'Routes'\npreferred_agents = ['claude', 'droid']\ndefault_agent = 'claude'\n").expect("team");
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    let store = Store::open_memory().expect("store");
    let args = RunArgs {
        team: Some("routes".into()),
        ..args()
    };
    let snapshot = report(Some(&store), &args);
    assert_eq!(snapshot.declared.difficulty, TaskDifficulty::Complex);
    assert_eq!(snapshot.inferred.kind, TaskCategory::Refactoring);
    assert!(
        snapshot
            .candidates
            .iter()
            .any(|c| c.agent == "claude" && c.eligible)
    );
    let selected = automatic_candidate(Some(&store), &args).expect("peer");
    assert_eq!(Some(selected.clone()), expected(&store, &args));
    assert_ne!(selected.agent, "claude");
    assert_ne!(selected.agent, "oz");
    let mut child = args.clone();
    apply_candidate(&mut child, &selected);
    assert_eq!(child.kind, args.kind);
    assert_eq!(child.team, args.team);
    assert_eq!(report(Some(&store), &child).declared, snapshot.declared);
}

#[test]
fn automatic_excludes_missing_disabled_auth_failed_below_floor_and_exhausted() {
    let (_dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![
        AgentKind::Codex,
        AgentKind::Droid,
        AgentKind::Qwen,
        AgentKind::Grok,
    ]);
    crate::agent_config::save_agent_disabled("grok", true).expect("disable");
    crate::auth_marker::record_failure_at(AgentKind::Droid, "Not signed in.", chrono::Local::now());
    let args = RunArgs {
        agent_name: "codex".into(),
        ..args()
    };
    let all = report(None, &args).candidates;
    let candidate = |name: &str| all.iter().find(|c| c.agent == name).expect("candidate");
    assert!(
        candidate("agy")
            .exclusion_codes
            .contains(&"not_installed".into())
    );
    assert!(
        candidate("droid")
            .exclusion_codes
            .contains(&"auth_failed".into())
    );
    assert!(
        candidate("qwen")
            .exclusion_codes
            .contains(&"below_floor".into())
    );
    assert!(!all.iter().any(|c| c.agent == "grok"));
    assert!(automatic_candidate(None, &args).is_none());
}

#[test]
fn automatic_excludes_selected_model_group_holds_even_background_and_advisory_rescue() {
    let (dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Droid, AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    hold(dir.path(), "droid--standard");
    let args = RunArgs {
        agent_name: "codex".into(),
        ..args()
    };
    let all = report(None, &args);
    assert_eq!(
        all.candidates
            .iter()
            .find(|c| c.agent == "droid")
            .expect("droid")
            .quota
            .status,
        "held"
    );
    assert!(automatic_candidate(None, &args).is_none());
    hold(dir.path(), "codex");
    assert!(
        report(None, &args).recommended.is_some(),
        "held routes can remain advisory"
    );
    assert!(automatic_candidate(None, &args).is_none());
}

#[test]
fn automatic_excludes_superseded_gemini_and_claude_but_explicit_gemini_still_runs() {
    let (dir, _home, _cache) = isolated();
    let _fleet = DetectAgentsGuard::set(vec![
        AgentKind::Gemini,
        AgentKind::Antigravity,
        AgentKind::Claude,
    ]);
    let store = Arc::new(Store::open_memory().expect("store"));
    let args = RunArgs {
        kind: Some(TaskCategory::Research),
        ..args()
    };
    assert!(report(Some(&store), &args).candidates.iter().any(|c|
        c.agent == "gemini" && c.exclusion_codes.contains(&"superseded_by_agy".into())));
    assert_eq!(
        automatic_candidate(Some(&store), &args)
            .expect("peer")
            .agent,
        "agy"
    );
    hold(dir.path(), "oz");
    let mut explicit = RunArgs {
        cascade: vec!["gemini".into(), "claude".into()],
        ..args
    };
    let setup =
        super::super::run_dispatch_resolve::resolve_agent_setup(&store, &mut explicit, None)
            .expect("explicit Gemini");
    assert_eq!(setup.agent_kind, AgentKind::Gemini);
    assert_eq!(explicit.cascade, ["claude"]);
    assert!(!explicit.advised_route);
}

#[path = "advice_route_resolve_tests.rs"]
mod resolve_tests;
