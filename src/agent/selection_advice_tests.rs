// Advise eligibility tests: install state, evidence state, auth, and caller pool.
// Covers: not-installed ineligible, unknown quota/auth, auth_failed, weaker/demoted same pool,
// and the advised model following the resolved agent default at standard budget.
// Deps: advise(), DetectAgentsGuard, auth_marker, isolated AID home and aidbar cache.

use super::*;
use crate::live_quota::CacheDirGuard;
use crate::paths::AidHomeGuard;
use crate::agent::run_model::RunModelSource;
use crate::types::{TaskDifficulty, TaskRigor, TaskUrgency};

fn isolated() -> (tempfile::TempDir, AidHomeGuard, CacheDirGuard) {
    let temp = tempfile::tempdir().expect("temp dir");
    let home = AidHomeGuard::set(temp.path());
    let cache = temp.path().join("aidbar");
    std::fs::create_dir_all(&cache).expect("cache dir");
    let guard = CacheDirGuard::set(&cache);
    // No test reads the developer's codex config (a readable CLI default).
    crate::agent::codex::cli_config::set_test_codex_home(Some(temp.path().join("no-codex")));
    (temp, home, guard)
}

fn declared(difficulty: TaskDifficulty, budget: TaskBudget) -> DeclaredTaskProfile {
    DeclaredTaskProfile { difficulty, budget, urgency: TaskUrgency::Normal, rigor: TaskRigor::Standard }
}

fn run(caller: Option<CallerAdvice>) -> AdviceReport {
    advise("refactor the scheduler", declared(TaskDifficulty::Moderate, TaskBudget::Standard),
        Some(TaskCategory::Refactoring), None, None, 0, caller)
}

fn anthropic_caller(capability: Option<f64>) -> CallerAdvice {
    CallerAdvice {
        session: "claude-code".to_string(),
        agent: "claude".to_string(),
        provider: "anthropic".to_string(),
        model: Some("caller-model".to_string()),
        capability,
    }
}

fn find<'a>(report: &'a AdviceReport, agent: &str) -> &'a AdviceCandidate {
    report.candidates.iter().find(|item| item.agent == agent).expect("candidate")
}

#[test]
fn not_installed_candidate_is_ineligible_with_reason() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    let report = run(None);
    let agy = find(&report, "agy");
    assert!(!agy.installed);
    assert!(!agy.eligible);
    assert!(agy.exclusion_codes.contains(&"not_installed".to_string()));
    assert!(agy.exclusion_reason.as_deref().is_some_and(|r| r.contains("not installed: binary 'agy'")));
    assert!(find(&report, "codex").eligible);
    assert_eq!(report.recommended.as_ref().map(|r| r.agent.as_str()), Some("codex"));
}

#[test]
fn no_evidence_reports_unknown_quota_and_auth() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Grok]);
    let grok = run(None).candidates.into_iter().find(|item| item.agent == "grok").expect("grok");
    assert_eq!(grok.quota.source, "none");
    assert_eq!(grok.quota.status, "unknown");
    assert_eq!(grok.auth.state, "unknown");
}

#[test]
fn observed_auth_failure_excludes_candidate() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Grok, AgentKind::Codex]);
    crate::auth_marker::record_failure_at(AgentKind::Grok, "Not signed in.", chrono::Local::now());
    let report = run(None);
    let grok = find(&report, "grok");
    assert_eq!(grok.auth.state, "failed");
    assert!(grok.auth.observed_at.is_some());
    assert!(!grok.eligible);
    assert!(grok.exclusion_codes.contains(&"auth_failed".to_string()));
}

#[test]
fn same_pool_weaker_model_is_excluded_with_known_caller_model() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude, AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("claude", Some("sonnet")).expect("sticky");
    crate::scores::test_support::seed_live();
    let report = run(Some(anthropic_caller(Some(99.0))));
    let claude = find(&report, "claude");
    assert_eq!(claude.model.as_deref(), Some("sonnet"), "a known model makes the comparison real");
    assert!(!claude.eligible);
    assert!(claude.exclusion_reason.as_deref().is_some_and(|r| r.contains("weaker model on caller's pool")));
    assert!(claude.exclusion_codes.contains(&"weaker_on_caller_pool".to_string()));
    assert_ne!(report.recommended.as_ref().map(|r| r.agent.as_str()), Some("claude"));
    assert_eq!(report.caller.as_ref().map(|c| c.provider.as_str()), Some("anthropic"));
}

#[test]
fn same_pool_is_demoted_not_excluded_with_unknown_caller_model() {
    let (_temp, _home, _cache) = isolated();
    let fleet = vec![AgentKind::Claude, AgentKind::Codex, AgentKind::Droid, AgentKind::Copilot];
    let _fleet = crate::agent::DetectAgentsGuard::set(fleet);
    crate::scores::test_support::seed_live();
    crate::agent_config::save_agent_default_model("claude", Some("opus")).expect("model");
    let baseline = run(None);
    assert_eq!(baseline.candidates[0].agent, "claude", "claude must outrank others without a caller");
    let report = run(Some(anthropic_caller(None)));
    let position = |agent: &str| report.candidates.iter().position(|c| c.agent == agent).expect("ranked");
    let claude = find(&report, "claude");
    assert!(claude.eligible);
    assert!(claude.demotion_reason.as_deref().is_some_and(|r| r.contains("same pool as caller")));
    let other_eligible = report.candidates.iter()
        .filter(|c| c.eligible && c.agent != "claude")
        .map(|c| position(&c.agent));
    assert!(other_eligible.clone().count() > 0);
    assert!(other_eligible.into_iter().all(|index| index < position("claude")));
    assert_ne!(report.recommended.as_ref().map(|r| r.agent.as_str()), Some("claude"));
}

fn codex_with_cli_default(budget: TaskBudget, cli_model: Option<&str>) -> AdviceCandidate {
    let (temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    let codex_home = temp.path().join("codex");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    if let Some(model) = cli_model {
        std::fs::write(codex_home.join("config.toml"), format!("model = \"{model}\"\n")).expect("config");
    }
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home));
    let declared = DeclaredTaskProfile {
        difficulty: TaskDifficulty::Moderate, budget,
        urgency: TaskUrgency::Normal, rigor: TaskRigor::Standard,
    };
    let report = advise("refactor the scheduler", declared, Some(TaskCategory::Refactoring), None, None, 0, None);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    find(&report, "codex").clone()
}

#[test]
fn standard_budget_advises_the_codex_cli_configured_default() {
    let codex = codex_with_cli_default(TaskBudget::Standard, Some("gpt-6-sol"));
    assert_eq!(codex.model.as_deref(), Some("gpt-6-sol"));
    assert!(!codex.pinned, "the CLI runs its own default; aid passes no -m");
    assert_eq!(codex.source, RunModelSource::CliConfig);
    assert_eq!(codex.breakdown.model_capability, 0.0, "unrated model: neutral base only");
    assert!(codex.eligible);
    assert!(codex.unrated_served_models.is_empty());
}

#[test]
fn unreadable_cli_default_is_reported_unknown_not_a_catalog_model() {
    let codex = codex_with_cli_default(TaskBudget::Standard, None);
    assert_eq!(codex.model, None);
    assert!(!codex.pinned);
    assert_eq!(codex.source, RunModelSource::AgentDefault);
    assert_eq!(codex.breakdown.model_capability, 0.0);
    let label = crate::agent::run_model::model_label(None, codex.pinned, codex.source);
    assert_eq!(label, "agent default (unknown)");
}

#[test]
fn cheap_budget_keeps_the_catalog_budget_model() {
    let codex = codex_with_cli_default(TaskBudget::Cheap, Some("gpt-6-sol"));
    let catalog = crate::model_catalog::model_for_task_budget(AgentKind::Codex, TaskBudget::Cheap);
    assert!(catalog.is_some());
    assert_eq!(codex.model.as_deref(), catalog);
    assert!(codex.pinned);
    assert_eq!(codex.source, RunModelSource::BudgetRoute);
}

#[test]
fn advised_model_group_hold_switches_route_but_other_group_does_not() {
    let (temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Droid, AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
    crate::agent_config::save_agent_default_model("codex", Some("gpt-6-sol")).expect("model");
    crate::scores::test_support::seed_live();
    let baseline = run(None).recommended.expect("recommendation");
    assert_eq!((&*baseline.agent, baseline.model.as_deref()), ("droid", Some("gpt-5.3-codex")));
    let hold = "hold: manual\nmessage: quota exhausted\n";
    std::fs::write(temp.path().join("rate-limit-droid--core"), hold).expect("other hold");
    let other = run(None);
    let recommended = other.recommended.expect("recommendation");
    assert_eq!((recommended.agent, recommended.model), (baseline.agent, baseline.model));
    assert_ne!(find(&run(None), "droid").quota.status, "held");
    assert!(find(&run(None), "droid").launchable(None));
    std::fs::remove_file(temp.path().join("rate-limit-droid--core")).expect("clear other hold");
    std::fs::write(temp.path().join("rate-limit-droid--standard"), hold).expect("model hold");
    let held = run(None);
    assert_eq!(find(&held, "droid").quota.status, "held");
    assert!(find(&held, "droid").eligible);
    assert!(!find(&held, "droid").launchable(None));
    let recommended = held.recommended.expect("recommendation");
    assert_eq!((&*recommended.agent, recommended.model.as_deref()), ("codex", Some("gpt-6-sol")));
}

#[path = "selection_advice_launch_tests.rs"]
mod launch_tests;

#[test]
fn claude_stays_listed_but_recommendation_requires_team_preference() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude, AgentKind::Codex]);
    let baseline = run(None);
    assert!(find(&baseline, "claude").installed);
    assert_eq!(baseline.recommended.expect("recommendation").agent, "codex");
    let team: TeamConfig = toml::from_str("id = 'preferred'\ndisplay_name = 'Preferred'\npreferred_agents = ['Claude']\n").expect("team");
    let report = advise("refactor the scheduler", baseline.declared, Some(TaskCategory::Refactoring), Some(&team), None, 0, None);
    assert_eq!(report.recommended.expect("recommendation").agent, "claude");
}

#[test]
fn only_claude_installed_without_preference_has_no_recommendation() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude]);
    let report = run(None);
    assert!(find(&report, "claude").installed);
    assert!(report.recommended.is_none());
}

#[test]
fn no_installed_routes_or_only_weaker_caller_pool_routes_have_no_recommendation() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![]);
    assert!(run(None).recommended.is_none());
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
    crate::agent_config::save_agent_default_model("codex", Some("openai/gpt-5.6-sol")).expect("model");
    crate::scores::test_support::seed_live();
    let caller = caller_advice("codex", Some("openai/gpt-5.6-sol"), TaskCategory::ComplexImpl).expect("caller");
    let caller = CallerAdvice { capability: Some(99.0), ..caller };
    let report = run(Some(caller));
    assert!(find(&report, "codex").exclusion_codes.contains(&"weaker_on_caller_pool".into()));
    assert!(report.recommended.is_none());
}

#[test]
fn research_and_frontend_advice_recommend_the_expected_installed_agent() {
    let (_temp, _home, _cache) = isolated();
    crate::scores::test_support::seed_live();
    for (prompt, fleet, expected) in [
        ("Explain the authentication flow and compare the docs?", [AgentKind::Gemini, AgentKind::Qwen], "gemini"),
        ("Explain the authentication flow and compare the docs?", [AgentKind::Antigravity, AgentKind::Qwen], "agy"),
        ("Create a responsive React component layout for the settings UI", [AgentKind::Cursor, AgentKind::Codex], "cursor"),
    ] {
        let _fleet = crate::agent::DetectAgentsGuard::set(fleet.to_vec());
        let model = match expected { "gemini" | "agy" => "gemini-3.7-flash-high", _ => "opus" };
        crate::agent_config::save_agent_default_model(expected, Some(model)).expect("model");
        let report = advise(prompt, declared(TaskDifficulty::Moderate, TaskBudget::Standard), None, None, None, 0, None);
        assert_eq!(report.recommended.expect("recommendation").agent, expected);
    }
}

#[test]
fn budget_simple_edit_advice_launches_eligible_budget_model() {
    let (_temp, _home, _cache) = isolated();
    for (fleet, expected) in [
        (vec![AgentKind::OpenCode, AgentKind::Kilo, AgentKind::Codex], AgentKind::OpenCode),
        // Kilo is unrated -> neutral base 6.0 >= floor 4; its free budget model wins over paid Codex (-3 penalty).
        (vec![AgentKind::Kilo, AgentKind::Codex], AgentKind::Kilo),
    ] {
        let _fleet = crate::agent::DetectAgentsGuard::set(fleet);
        let report = advise("rename src/types.rs field name", declared(TaskDifficulty::Simple, TaskBudget::Free),
            None, None, None, 0, None);
        let picked = report.recommended.expect("recommendation");
        assert_eq!(picked.agent, expected.as_str());
        let model = crate::model_catalog::budget_model(&expected).expect("catalog budget model");
        assert_eq!(picked.model.as_deref(), Some(model));
        assert_eq!(picked.source, RunModelSource::BudgetRoute);
        assert!(picked.pinned);
    }
}

#[test]
fn team_override_changes_the_advised_agent() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Gemini, AgentKind::OpenCode]);
    let profile = declared(TaskDifficulty::Simple, TaskBudget::Standard);
    let prompt = "rename src/types.rs field name to task_name";
    assert_eq!(advise(prompt, profile, None, None, None, 0, None).recommended.expect("baseline").agent, "opencode");
    let team: TeamConfig = toml::from_str("id = 'override'\ndisplay_name = 'Override'\npreferred_agents = []\n[overrides.gemini]\nsimple_edit = 10\n").expect("team");
    let picked = advise(prompt, profile, None, Some(&team), None, 0, None).recommended.expect("override");
    assert_eq!((picked.agent.as_str(), picked.model.as_deref()), ("gemini", None));
}

#[test]
fn advice_skips_disabled_installed_agents() {
    let (_temp, _home, _cache) = isolated();
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Gemini, AgentKind::Qwen]);
    crate::agent_config::save_agent_disabled("gemini", true).expect("disable");
    let report = advise("Explain the authentication flow and compare the docs?",
        declared(TaskDifficulty::Moderate, TaskBudget::Standard), None, None, None, 0, None);
    assert_eq!(report.recommended.expect("recommendation").agent, "qwen");
    assert!(report.candidates.iter().all(|candidate| candidate.agent != "gemini"));
}

#[path = "selection_evidence_tests.rs"]
mod evidence_tests;
