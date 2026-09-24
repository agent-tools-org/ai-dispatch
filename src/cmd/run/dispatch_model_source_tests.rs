// Dispatch model-source regression tests.
// Exports: resolver provenance coverage.
// Deps: run resolver, model validation, RunArgs, Store.

use super::super::resolve_agent_setup;
use crate::agent::model_validation::{ModelSource, MockServedModelsGuard};
use crate::cmd::run::RunArgs;
use crate::paths::AidHomeGuard;
use crate::store::Store;
use crate::types::AgentKind;
use std::sync::Arc;

#[test]
fn aid_selected_default_is_persisted_as_aid_resolved() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    crate::agent_config::save_agent_default_model("grok", Some("stale-aid-model"))
        .expect("save agent default");
    let _served = MockServedModelsGuard::set(
        AgentKind::Grok,
        Some(vec!["grok-4.7".to_string()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "say hi".to_string(),
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args, None).expect("aid-selected model may degrade");

    assert_eq!(setup.effective_model, None);
    assert_eq!(args.model_source, ModelSource::AidResolved);
    let restored = RunArgs::from_dispatch_args_json(&args.dispatch_args_json().expect("serialize"))
        .expect("restore dispatch args");
    assert_eq!(restored.model_source, ModelSource::AidResolved);
}

#[test]
fn explicit_model_remains_user_supplied() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    let _served = MockServedModelsGuard::set(
        AgentKind::Grok,
        Some(vec!["explicit-model".to_string()]),
    );
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "grok".to_string(),
        prompt: "say hi".to_string(),
        model: Some("explicit-model".to_string()),
        model_source: ModelSource::UserSupplied,
        ..Default::default()
    };

    resolve_agent_setup(&store, &mut args, None).expect("served explicit model");

    assert_eq!(args.model_source, ModelSource::UserSupplied);
}

#[test]
fn substituted_model_is_persisted_as_aid_resolved() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _home_guard = AidHomeGuard::set(home.path());
    crate::rate_limit::mark_group_rate_limited(
        &AgentKind::Cursor,
        None,
        "premium",
        "premium quota exhausted",
    );
    let _served = MockServedModelsGuard::set(AgentKind::Cursor, Some(vec!["auto".to_string()]));
    let store = Arc::new(Store::open_memory().expect("store"));
    let mut args = RunArgs {
        agent_name: "cursor".to_string(),
        prompt: "say hi".to_string(),
        model: Some("composer-2.5".to_string()),
        model_source: ModelSource::UserSupplied,
        ..Default::default()
    };

    let setup = resolve_agent_setup(&store, &mut args, None).expect("healthy replacement is usable");

    assert_eq!(setup.effective_model.as_deref(), Some("auto"));
    assert_eq!(args.model_source, ModelSource::AidResolved);
    let restored = RunArgs::from_dispatch_args_json(&args.dispatch_args_json().expect("serialize"))
        .expect("restore dispatch args");
    assert_eq!(restored.model_source, ModelSource::AidResolved);
}

#[test]
fn declared_standard_and_premium_reach_cli_default() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _guard = AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    for agent_name in ["codex", "agy"] {
        for budget in [crate::types::TaskBudget::Standard, crate::types::TaskBudget::Premium] {
            let mut run_args = RunArgs {
                agent_name: agent_name.to_string(),
                prompt: "Refactor validation".to_string(),
                declared_difficulty: Some(crate::types::TaskDifficulty::Moderate),
                declared_budget: Some(budget),
                ..Default::default()
            };
            assert_cli_default(&store, &mut run_args);
        }
    }
}

fn assert_cli_default(store: &Arc<Store>, args: &mut RunArgs) {
    assert_eq!(crate::agent_config::get_default_model(&args.agent_name), None);
    assert_eq!(args.model, None);
    let codex_home = tempfile::tempdir().expect("empty codex home");
    crate::agent::codex::cli_config::set_test_codex_home(Some(codex_home.path().to_path_buf()));
    let setup = resolve_agent_setup(store, args, None);
    crate::agent::codex::cli_config::set_test_codex_home(None);
    let setup = setup.expect("healthy default dispatch");
    assert_eq!(setup.effective_model, None);
    let unpinned = crate::agent::run_model::RunModel {
        model: None, pinned: false, source: crate::agent::run_model::RunModelSource::AgentDefault,
    };
    let info = super::super::model_info::model_selection_info(
        args, &unpinned, setup.effective_model.as_deref(), setup.agent.as_ref(),
    );
    assert_eq!(info, format!(
        "[aid] {} model: CLI default (no -m); source: CLI default (no -m)", args.agent_name,
    ));
}

#[test]
fn healthy_defaults_emit_source_without_exhausted_warning() {
    let output = std::process::Command::new(std::env::current_exe().expect("test executable"))
        .args(["declared_standard_and_premium_reach_cli_default", "--nocapture"])
        .output().expect("run isolated resolver test");
    assert!(output.status.success(), "{output:?}");
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(stderr.matches("source: CLI default (no -m)").count(), 4, "{stderr}");
    assert!(!stderr.contains("exhausted"), "{stderr}");
}

#[test]
fn declared_standard_and_premium_do_not_trigger_simple_task_smart_routing() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _guard = AidHomeGuard::set(home.path());
    let store = Arc::new(Store::open_memory().expect("store"));
    for budget in [crate::types::TaskBudget::Standard, crate::types::TaskBudget::Premium] {
        for difficulty in [crate::types::TaskDifficulty::Trivial, crate::types::TaskDifficulty::Simple] {
            let mut args = RunArgs {
                agent_name: "codex".to_string(),
                prompt: "Fix a typo".to_string(),
                declared_difficulty: Some(difficulty),
                declared_budget: Some(budget),
                ..Default::default()
            };
            assert_cli_default(&store, &mut args);
        }
    }
}

#[test]
fn model_info_names_final_model_and_precedence_source() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _guard = AidHomeGuard::set(home.path());
    let mut args = RunArgs {
        agent_name: "gemini".to_string(),
        model: Some("flash".to_string()),
        declared_budget: Some(crate::types::TaskBudget::Cheap),
        ..Default::default()
    };
    use crate::agent::run_model::{RunModel, RunModelSource};
    let resolved = |model: &str, source| RunModel { model: Some(model.to_string()), pinned: true, source };
    let info = |args: &RunArgs, run_model: &RunModel, model: Option<&str>|
        super::super::model_info::model_selection_info(args, run_model, model, &crate::agent::gemini::GeminiAgent);
    let explicit = resolved("flash", RunModelSource::Explicit);
    assert_eq!(info(&args, &explicit, Some("flash")), "[aid] gemini model: flash; source: --model");
    args.model_source = ModelSource::AidResolved;
    let sticky = resolved("pro", RunModelSource::Sticky);
    assert_eq!(info(&args, &sticky, Some("pro")), "[aid] gemini model: pro; source: agent config");
    let budget = resolved("flash-lite", RunModelSource::BudgetRoute);
    assert_eq!(info(&args, &budget, Some("flash-lite")),
        "[aid] gemini model: flash-lite; source: budget route");
    assert_eq!(info(&args, &budget, Some("other-family")),
        "[aid] gemini model: other-family; source: quota/budget routing");
    let unpinned = RunModel { model: None, pinned: false, source: RunModelSource::AgentDefault };
    assert!(info(&args, &unpinned, None).ends_with("source: CLI default (no -m)"));
}

#[test]
fn model_info_reports_existing_adapter_defaults_accurately() {
    let home = tempfile::tempdir().expect("temporary aid home");
    let _guard = AidHomeGuard::set(home.path());
    for kind in [AgentKind::Cursor, AgentKind::Qwen, AgentKind::MiMoCode] {
        let args = RunArgs { agent_name: kind.as_str().to_string(), ..Default::default() };
        let agent = crate::agent::get_agent(kind);
        let opts = crate::agent::RunOpts {
            dir: None, output: None, result_file: None, model: None, budget: false,
            read_only: false, sandbox: false, context_files: vec![], session_id: None,
            env: None, env_forward: None,
        };
        let command = agent.build_command("say hi", &opts)
            .expect("build adapter command");
        let command_args: Vec<_> = command.get_args().map(|arg| arg.to_string_lossy()).collect();
        let model = command_args.windows(2)
            .find(|pair| pair[0] == "-m" || pair[0] == "--model")
            .expect("adapter model flag")[1].as_ref();
        assert_eq!(agent.default_model().as_deref(), Some(model));
        let unpinned = crate::agent::run_model::RunModel {
            model: None, pinned: false, source: crate::agent::run_model::RunModelSource::AgentDefault,
        };
        let info = super::super::model_info::model_selection_info(&args, &unpinned, None, agent.as_ref());
        assert_eq!(info, format!(
            "[aid] {} model: {model}; source: adapter default (no caller -m)", kind.as_str(),
        ));
    }
}
