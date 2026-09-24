// One test per RunModelSource, plus the unknown-default label.
// Deps: resolve_run_model, isolated AID_HOME, test codex home, custom agent config parser.

use super::*;
use crate::paths::AidHomeGuard;

struct Isolated {
    _temp: tempfile::TempDir,
    _home: AidHomeGuard,
    codex_home: std::path::PathBuf,
}

/// Isolated AID home and an empty codex home (no readable CLI default).
fn isolated() -> Isolated {
    let temp = tempfile::tempdir().expect("tempdir");
    let home = AidHomeGuard::set(temp.path());
    let codex_home = temp.path().join("codex");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home.clone()));
    Isolated { _temp: temp, _home: home, codex_home }
}

fn input(kind: AgentKind) -> RunModelInput<'static> {
    RunModelInput {
        agent_name: kind.as_str(), kind, explicit_model: None, force_default: false, custom: None,
        budget_mode: false, declared_budget: None, declared_difficulty: None, smart_routing: true,
    }
}

fn pinned(model: &str, source: RunModelSource) -> RunModel {
    RunModel { model: Some(model.to_string()), pinned: true, source }
}

#[test]
fn explicit_model_is_pinned_over_sticky() {
    let _env = isolated();
    crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("sticky");
    let resolved = resolve_run_model(&RunModelInput {
        explicit_model: Some("flash"), declared_budget: Some(TaskBudget::Cheap), ..input(AgentKind::Gemini)
    });
    assert_eq!(resolved, pinned("flash", RunModelSource::Explicit));
}

#[test]
fn forced_default_passes_nothing_even_with_explicit_model() {
    let _env = isolated();
    let resolved = resolve_run_model(&RunModelInput {
        explicit_model: Some("gone-model"), force_default: true, budget_mode: true,
        ..input(AgentKind::Codex)
    });
    assert_eq!(resolved, RunModel { model: None, pinned: false, source: RunModelSource::ForcedDefault });
}

#[test]
fn sticky_default_beats_declared_cheap_catalog() {
    let _env = isolated();
    crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("sticky");
    let resolved = resolve_run_model(&RunModelInput {
        declared_budget: Some(TaskBudget::Cheap), budget_mode: true, ..input(AgentKind::Gemini)
    });
    assert_eq!(resolved, pinned("pro", RunModelSource::Sticky));
}

#[test]
fn custom_forced_model_is_reported_not_pinned() {
    let _env = isolated();
    let config = crate::agent::custom::parse_config(
        "[agent]\nid = 'byok'\ndisplay_name = 'BYOK'\ncommand = 'opencode'\n\
         delegate_to = 'opencode'\nforced_model = 'provider/custom-model'\n",
    ).expect("custom config");
    let resolved = resolve_run_model(&RunModelInput {
        agent_name: "byok", custom: Some(&config), budget_mode: true, ..input(AgentKind::Custom)
    });
    let expected = RunModel {
        model: Some("provider/custom-model".to_string()), pinned: false,
        source: RunModelSource::CustomForced,
    };
    assert_eq!(resolved, expected, "the adapter applies forced_model; aid passes no -m");
}

#[test]
fn smart_route_pins_budget_model_for_declared_simple_task_without_budget() {
    let _env = isolated();
    let simple = RunModelInput { declared_difficulty: Some(TaskDifficulty::Simple), ..input(AgentKind::Gemini) };
    let expected = crate::model_catalog::budget_model(&AgentKind::Gemini).expect("gemini budget model");
    assert_eq!(resolve_run_model(&simple), pinned(expected, RunModelSource::SmartRoute));
    let declared = RunModelInput { declared_budget: Some(TaskBudget::Standard), ..simple };
    assert_eq!(resolve_run_model(&declared).source, RunModelSource::AgentDefault);
    let disabled = RunModelInput { smart_routing: false, ..simple };
    assert_eq!(resolve_run_model(&disabled).source, RunModelSource::AgentDefault);
}

#[test]
fn budget_route_pins_declared_cheap_catalog_and_budget_mode_model() {
    let _env = isolated();
    let cheap = RunModelInput { declared_budget: Some(TaskBudget::Cheap), budget_mode: true, ..input(AgentKind::Gemini) };
    assert_eq!(resolve_run_model(&cheap), pinned("flash-lite", RunModelSource::BudgetRoute));
    let grok = RunModelInput { declared_budget: Some(TaskBudget::Cheap), budget_mode: true, ..input(AgentKind::Grok) };
    assert_eq!(resolve_run_model(&grok), pinned("grok-4.6", RunModelSource::BudgetRoute));
    let mode = RunModelInput { budget_mode: true, ..input(AgentKind::Gemini) };
    let expected = crate::model_catalog::budget_model(&AgentKind::Gemini).expect("gemini budget model");
    assert_eq!(resolve_run_model(&mode), pinned(expected, RunModelSource::BudgetRoute));
}

#[test]
fn readable_cli_default_is_cli_config_and_not_pinned() {
    let env = isolated();
    std::fs::write(env.codex_home.join("config.toml"), "model = \"gpt-6-sol\"\n").expect("config");
    let resolved = resolve_run_model(&RunModelInput {
        declared_budget: Some(TaskBudget::Premium), ..input(AgentKind::Codex)
    });
    let expected = RunModel { model: Some("gpt-6-sol".to_string()), pinned: false, source: RunModelSource::CliConfig };
    assert_eq!(resolved, expected);
    assert_eq!(model_label(resolved.model.as_deref(), false, resolved.source), "gpt-6-sol (cli_config, not pinned)");
}

#[test]
fn nothing_known_is_agent_default_with_no_model() {
    let _env = isolated();
    for kind in [AgentKind::Codex, AgentKind::Claude] {
        let resolved = resolve_run_model(&RunModelInput {
            declared_budget: Some(TaskBudget::Standard), ..input(kind)
        });
        assert_eq!(resolved, RunModel { model: None, pinned: false, source: RunModelSource::AgentDefault });
    }
    assert_eq!(model_label(None, false, RunModelSource::AgentDefault), "agent default (unknown)");
}

#[test]
fn source_serializes_as_its_snake_case_name() {
    for source in [
        RunModelSource::Explicit, RunModelSource::ForcedDefault, RunModelSource::Sticky,
        RunModelSource::CustomForced, RunModelSource::SmartRoute, RunModelSource::BudgetRoute,
        RunModelSource::CliConfig, RunModelSource::AgentDefault,
    ] {
        let json = serde_json::to_value(source).expect("serialize");
        assert_eq!(json, source.as_str());
    }
}
