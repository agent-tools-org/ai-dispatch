// Advise eligibility tests: install state, evidence state, auth, and caller pool.
// Covers: not-installed ineligible, unknown quota/auth, auth_failed, weaker/demoted same pool,
// and the advised model following the resolved agent default at standard budget.
// Deps: advise(), DetectAgentsGuard, auth_marker, isolated AID home and aidbar cache.

use super::*;
use crate::live_quota::CacheDirGuard;
use crate::paths::AidHomeGuard;
use crate::agent::run_model::RunModelSource;
use crate::types::{TaskRigor, TaskUrgency};

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

fn run(caller: Option<CallerAdvice>) -> AdviceReport {
    let declared = DeclaredTaskProfile {
        difficulty: TaskDifficulty::Moderate,
        budget: TaskBudget::Standard,
        urgency: TaskUrgency::Normal,
        rigor: TaskRigor::Standard,
    };
    advise("refactor the scheduler", declared, Some(TaskCategory::Refactoring), None, None, 0, caller)
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
    assert_eq!(codex.breakdown.model_capability, 0.0, "unrated model: agent-level base only");
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
