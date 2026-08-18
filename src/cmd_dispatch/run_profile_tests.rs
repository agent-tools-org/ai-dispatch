// Isolated AID_HOME coverage for explicit_agent model precedence.
// Exports: (tests only)
// Deps: explicit_agent, AidHomeGuard, agent_config, TaskBudget, TaskEgress.

use super::*;
use crate::paths::AidHomeGuard;
use crate::types::TaskEgress;

fn isolated_home() -> (tempfile::TempDir, AidHomeGuard) {
    let temp = tempfile::tempdir().expect("tempdir");
    let guard = AidHomeGuard::set(temp.path());
    (temp, guard)
}

fn dispatch_model(
    agent: &str,
    model: Option<&str>,
    budget: Option<TaskBudget>,
) -> Option<String> {
    let cli = model.map(str::to_string);
    let (_, auto) = explicit_agent(agent.into(), &cli, budget, TaskEgress::Any)
        .expect("explicit agent must dispatch");
    cli.or(auto)
}

#[test]
fn rigor_no_longer_gates_agent_identity() {
    let (_temp, _guard) = isolated_home();
    let result = explicit_agent(
        "claude".into(),
        &None,
        Some(TaskBudget::Standard),
        TaskEgress::Any,
    );
    assert!(result.is_ok(), "identity gate must be gone: {result:?}");
}

#[test]
fn declared_budget_is_preference_not_hard_gate() {
    let (_temp, _guard) = isolated_home();
    // Claude has no free-tier catalog model. Pre-fix this bailed; budget
    // is a preference so dispatch must still succeed.
    let result = explicit_agent(
        "claude".into(),
        &None,
        Some(TaskBudget::Free),
        TaskEgress::Any,
    );
    assert!(
        result.is_ok(),
        "declared budget must not refuse dispatch: {result:?}"
    );
}

#[test]
fn cheap_budget_dispatches_grok_with_cli_default_model() {
    let (_temp, _guard) = isolated_home();
    let (agent, model) = explicit_agent(
        "grok".into(),
        &None,
        Some(TaskBudget::Cheap),
        TaskEgress::Any,
    )
    .expect("grok --budget cheap must dispatch");
    assert_eq!(agent, "grok");
    assert_eq!(model.as_deref(), Some("grok-4.6"));
}

#[test]
fn cheap_budget_dispatches_gemini_flash_lite() {
    let (_temp, _guard) = isolated_home();
    let (agent, model) = explicit_agent(
        "gemini".into(),
        &None,
        Some(TaskBudget::Cheap),
        TaskEgress::Any,
    )
    .expect("gemini --budget cheap must dispatch");
    assert_eq!(agent, "gemini");
    assert_eq!(model.as_deref(), Some("flash-lite"));
}

#[test]
fn egress_local_refuses_builtin_third_party() {
    let (_temp, _guard) = isolated_home();
    let err = explicit_agent(
        "codex".into(),
        &None,
        Some(TaskBudget::Standard),
        TaskEgress::Local,
    )
    .expect_err("codex must fail --egress local");
    assert!(err.to_string().contains("--egress local"));
}

#[test]
fn egress_any_admits_third_party() {
    let (_temp, _guard) = isolated_home();
    assert!(explicit_agent(
        "codex".into(),
        &None,
        Some(TaskBudget::Standard),
        TaskEgress::Any,
    )
    .is_ok());
}

#[test]
fn egress_private_network_refuses_public_third_party() {
    let (_temp, _guard) = isolated_home();
    let err = explicit_agent(
        "codex".into(),
        &None,
        Some(TaskBudget::Standard),
        TaskEgress::PrivateNetwork,
    )
    .expect_err("codex must fail --egress private-network");
    assert!(err.to_string().contains("--egress private-network"));
}

#[test]
fn configured_default_model_wins_over_catalog_default() {
    let (_temp, _guard) = isolated_home();
    crate::agent_config::save_agent_default_model("opencode", Some("opencode/kimi-k2.6"))
        .expect("save config");

    let (_agent, model) = explicit_agent(
        "opencode".into(),
        &None,
        Some(TaskBudget::Standard),
        TaskEgress::Any,
    )
    .expect("opencode dispatch must resolve");
    assert_eq!(
        model.as_deref(),
        Some("opencode/kimi-k2.6"),
        "configured default model must be used, not the catalog default opencode/glm-5.2"
    );
}

#[test]
fn configured_default_wins_over_declared_cheap_budget() {
    let (_temp, _guard) = isolated_home();
    crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("save config");
    assert_eq!(
        dispatch_model("gemini", None, Some(TaskBudget::Cheap)).as_deref(),
        Some("pro"),
        "configured default must outrank catalog cheap pick flash-lite"
    );
}

#[test]
fn cli_model_beats_configured_default() {
    let (_temp, _guard) = isolated_home();
    crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("save config");
    assert_eq!(
        dispatch_model("gemini", Some("flash"), Some(TaskBudget::Cheap)).as_deref(),
        Some("flash"),
        "--model must beat a configured default"
    );
}

#[test]
fn catalog_is_used_when_no_default_is_configured() {
    let (_temp, _guard) = isolated_home();
    assert_eq!(
        dispatch_model("gemini", None, Some(TaskBudget::Cheap)).as_deref(),
        Some("flash-lite"),
        "empty agent_config must still use the catalog cheap pick"
    );
}
