// Table test: `aid run` and `aid advise` resolve an identical RunModel for one profile.
// Deps: dispatch_run_model::input, RunModelInput::declared, isolated AID_HOME and codex home.

use super::*;
use crate::paths::AidHomeGuard;
use crate::types::{DeclaredTaskProfile, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

#[test]
fn run_and_advise_resolve_identical_models_for_the_same_profile() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = AidHomeGuard::set(temp.path());
    let codex_home = temp.path().join("codex");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::write(codex_home.join("config.toml"), "model = \"gpt-6-sol\"\n").expect("config");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home));
    crate::agent_config::save_agent_default_model("claude", Some("sonnet")).expect("sticky");
    let selection = SelectionConfig::default();
    let mut compared = 0;
    for kind in [AgentKind::Codex, AgentKind::Gemini, AgentKind::Grok, AgentKind::Claude, AgentKind::Qwen] {
        for difficulty in [TaskDifficulty::Trivial, TaskDifficulty::Moderate, TaskDifficulty::Complex] {
            for budget in [TaskBudget::Free, TaskBudget::Cheap, TaskBudget::Standard, TaskBudget::Premium] {
                let declared = DeclaredTaskProfile {
                    difficulty, budget, urgency: TaskUrgency::Normal, rigor: TaskRigor::Standard,
                };
                // What `aid run --difficulty .. --budget ..` builds (run_batch budget_mode).
                let args = RunArgs {
                    agent_name: kind.as_str().to_string(),
                    budget: budget.uses_budget_mode() || selection.budget_mode,
                    declared_difficulty: Some(difficulty),
                    declared_budget: Some(budget),
                    ..Default::default()
                };
                let run = resolve_run_model(&input(&args, kind, None, false, &selection));
                let advise = resolve_run_model(&RunModelInput::declared(
                    kind.as_str(), kind, None, declared, &selection,
                ));
                assert_eq!(run, advise, "{kind:?} {difficulty:?} {budget:?}");
                compared += 1;
            }
        }
    }
    crate::agent::codex::cli_config::set_test_codex_home(None);
    assert_eq!(compared, 60);
}

#[test]
fn run_passes_the_model_to_the_cli_only_when_pinned() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = AidHomeGuard::set(temp.path());
    let codex_home = temp.path().join("codex");
    std::fs::create_dir_all(&codex_home).expect("codex home");
    std::fs::write(codex_home.join("config.toml"), "model = \"gpt-6-sol\"\n").expect("config");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home));
    let store = std::sync::Arc::new(crate::store::Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "codex".to_string(), prompt: "Refactor".to_string(),
        declared_budget: Some(TaskBudget::Standard), ..Default::default()
    };
    let setup = super::super::resolve_agent_setup(&store, &mut args, None);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    assert_eq!(setup.expect("dispatch setup").effective_model, None, "cli_config is not pinned");
}
