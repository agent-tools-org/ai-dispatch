// Final dispatch model attribution after quota routing and served-model validation.
// Exports: model_selection_info; deps: Agent, RunArgs, agent_config, model_catalog.

use super::RunArgs;
use crate::agent::model_validation::ModelSource;
use crate::types::{AgentKind, TaskBudget};

pub(super) fn model_selection_info(
    args: &RunArgs, effective_model: Option<&str>, agent: &dyn crate::agent::Agent,
) -> String {
    let adapter_default = effective_model.is_none().then(|| agent.default_model()).flatten();
    let custom = agent.kind() == AgentKind::Custom;
    let source = match effective_model {
        None if args.force_default_model => match (custom, adapter_default.is_some()) {
            (true, false) => "self-heal retry: custom/delegate default (unknown)",
            (true, true) => "self-heal retry: custom/delegate default",
            (false, true) => "self-heal retry: adapter default",
            (false, false) => "self-heal retry: CLI default",
        },
        None if custom && adapter_default.is_none() => "custom/delegate default (unknown)",
        None if custom => "custom/delegate default (no caller -m)",
        None if adapter_default.is_some() => "adapter default (no caller -m)",
        None => "CLI default (no -m)",
        Some(model) if args.model_source == ModelSource::UserSupplied
            && args.model.as_deref() == Some(model) => "--model",
        Some(model) if crate::agent_config::get_default_model(&args.agent_name).as_deref()
            == Some(model) => "agent config",
        Some(model) if matches!(args.declared_budget, Some(TaskBudget::Free | TaskBudget::Cheap))
            && AgentKind::parse_str(&args.agent_name).is_some_and(|kind| {
                args.declared_budget.and_then(|budget| {
                    crate::model_catalog::model_for_task_budget(kind, budget)
                }) == Some(model)
            }) => "catalog (declared budget)",
        Some(_) => "quota/budget routing",
    };
    let unknown_default = if custom {
        "unknown (custom/delegate default)"
    } else if args.force_default_model {
        "CLI default"
    } else {
        "CLI default (no -m)"
    };
    format!(
        "[aid] {} model: {}; source: {source}",
        args.agent_name,
        effective_model.or(adapter_default.as_deref()).unwrap_or(unknown_default)
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn self_heal_retry_labels_cli_default_even_with_original_model() {
        let args = RunArgs {
            agent_name: "codex".to_string(),
            model: Some("unavailable-model".to_string()),
            force_default_model: true,
            ..Default::default()
        };
        assert_eq!(model_selection_info(&args, None, &crate::agent::codex::CodexAgent),
            "[aid] codex model: CLI default; source: self-heal retry: CLI default");
    }

    #[test]
    fn custom_default_is_unknown_instead_of_cli_default() {
        let args = RunArgs { agent_name: "byok".to_string(), ..Default::default() };
        let agent = crate::agent::custom::CustomAgent {
            config: crate::agent::custom::parse_config(
                "[agent]\nid = 'byok'\ndisplay_name = 'BYOK'\ncommand = 'wrapper'\ndelegate_to = 'opencode'\n",
            ).expect("custom config"),
        };
        assert_eq!(model_selection_info(&args, None, &agent),
            "[aid] byok model: unknown (custom/delegate default); source: custom/delegate default (unknown)");
    }

    #[test]
    fn custom_delegate_reports_its_actual_forced_default() {
        let home = tempfile::tempdir().expect("temporary aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        std::fs::create_dir(home.path().join("agents")).expect("agents directory");
        std::fs::write(home.path().join("agents/byok.toml"),
            "[agent]\nid = 'byok'\ndisplay_name = 'BYOK'\ncommand = 'opencode'\ndelegate_to = 'opencode'\nforced_model = 'provider/custom-model'\n",
        ).expect("delegate config");
        let agent = crate::agent::registry::resolve_custom_agent("byok").expect("delegate adapter");
        let args = RunArgs { agent_name: "byok".to_string(), ..Default::default() };
        assert_eq!(model_selection_info(&args, None, agent.as_ref()),
            "[aid] byok model: provider/custom-model; source: custom/delegate default (no caller -m)");
    }

    #[test]
    fn self_heal_retry_reports_adapter_default() {
        let args = RunArgs {
            agent_name: "cursor".to_string(),
            model: Some("unavailable-model".to_string()),
            force_default_model: true,
            ..Default::default()
        };
        assert_eq!(model_selection_info(&args, None, &crate::agent::cursor::CursorAgent),
            "[aid] cursor model: composer-2.5; source: self-heal retry: adapter default");
    }

    #[test]
    fn self_heal_retry_reports_known_delegate_default() {
        let agent = crate::agent::opencode_overlay::OpenCodeOverlayAgent::new(
            "byok".to_string(), "BYOK".to_string(), "provider/custom-model".to_string(),
        );
        let args = RunArgs {
            agent_name: "byok".to_string(), force_default_model: true, ..Default::default()
        };
        assert_eq!(model_selection_info(&args, None, &agent),
            "[aid] byok model: provider/custom-model; source: self-heal retry: custom/delegate default");
    }

    #[test]
    fn self_heal_retry_keeps_unknown_delegate_default_unknown() {
        let agent = crate::agent::custom::CustomAgent {
            config: crate::agent::custom::parse_config(
                "[agent]\nid = 'byok'\ndisplay_name = 'BYOK'\ncommand = 'wrapper'\n",
            ).expect("custom config"),
        };
        let args = RunArgs {
            agent_name: "byok".to_string(), force_default_model: true, ..Default::default()
        };
        assert_eq!(model_selection_info(&args, None, &agent),
            "[aid] byok model: unknown (custom/delegate default); source: self-heal retry: custom/delegate default (unknown)");
    }

    #[test]
    fn routed_retry_model_keeps_its_source() {
        let home = tempfile::tempdir().expect("temporary aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        let args = RunArgs {
            agent_name: "cursor".to_string(), force_default_model: true, ..Default::default()
        };
        assert_eq!(model_selection_info(&args, Some("auto"), &crate::agent::cursor::CursorAgent),
            "[aid] cursor model: auto; source: quota/budget routing");
    }

    #[test]
    fn qwen_default_follows_selected_settings_model() {
        let home = tempfile::tempdir().expect("temporary qwen home");
        std::fs::create_dir(home.path().join(".qwen")).expect("qwen directory");
        std::fs::write(home.path().join(".qwen/settings.json"),
            r#"{"model":{"name":"configured-qwen"}}"#).expect("qwen settings");
        crate::model_catalog::set_test_qwen_home(Some(home.path().to_path_buf()));
        let args = RunArgs { agent_name: "qwen".to_string(), ..Default::default() };
        let info = model_selection_info(&args, None, &crate::agent::qwen::QwenAgent);
        crate::model_catalog::set_test_qwen_home(None);
        assert_eq!(info,
            "[aid] qwen model: configured-qwen; source: adapter default (no caller -m)");
    }
}
