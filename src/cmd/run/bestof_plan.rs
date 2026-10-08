// Best-of plans retain advice's ranked builtin routes and resolved models.
// Exports plan construction and racer arguments; deps: advise, RunArgs, team policy.

use anyhow::{bail, Result};
use crate::cmd::run::RunArgs;
#[cfg(test)]
use crate::agent::model_validation::ModelSource;
use crate::agent::selection::AdviceCandidate;
use crate::store::Store;
use crate::team::TeamConfig;
use crate::types::DeclaredTaskProfile;

pub(super) fn advised_plan(
    store: &Store, args: &RunArgs, n: usize,
) -> Result<(DeclaredTaskProfile, Vec<AdviceCandidate>)> {
    let report = super::super::advice_route::report(Some(store), args);
    let declared = report.declared;
    let team = args.team.as_deref().and_then(crate::team::resolve_team);
    Ok((declared, expand_best_of_plan(report.candidates, team.as_ref(), n)?))
}

fn expand_best_of_plan(
    candidates: Vec<AdviceCandidate>, team: Option<&TeamConfig>, n: usize,
) -> Result<Vec<AdviceCandidate>> {
    let mut plan: Vec<_> = candidates.into_iter().filter(|c| c.launchable(team)).take(n).collect();
    if plan.is_empty() {
        bail!("best-of-{n}: no launchable advise candidates");
    }
    let base_len = plan.len();
    while plan.len() < n {
        plan.push(plan[plan.len() % base_len].clone());
    }
    Ok(plan)
}

pub(super) fn racer_args(
    args: &RunArgs, candidate: &AdviceCandidate,
) -> RunArgs {
    let mut child = args.clone();
    super::super::advice_route::apply_candidate(&mut child, candidate);
    child.background = true;
    child.judge = None;
    child.announce = false;
    child.best_of = None;
    child
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::classifier::TaskCategory;
    use crate::agent::run_model::{RunModelSource};
    use crate::agent::selection::{advise, caller_advice};
    use crate::types::{AgentKind, TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};

    fn isolated() -> (tempfile::TempDir, crate::paths::AidHomeGuard, crate::live_quota::CacheDirGuard) {
        let temp = tempfile::tempdir().expect("home");
        let home = crate::paths::AidHomeGuard::set(temp.path());
        let cache = crate::live_quota::CacheDirGuard::set(temp.path());
        crate::agent::codex::cli_config::set_test_codex_home(Some(temp.path().join("no-codex")));
        (temp, home, cache)
    }

    fn profile() -> DeclaredTaskProfile {
        DeclaredTaskProfile {
            difficulty: TaskDifficulty::Moderate, budget: TaskBudget::Standard,
            urgency: TaskUrgency::Normal, rigor: TaskRigor::Standard,
        }
    }

    fn candidates() -> Vec<AdviceCandidate> {
        advise("refactor the scheduler", profile(), Some(TaskCategory::Refactoring), None, None, 0, None).candidates
    }

    fn hold(home: &std::path::Path, route: &str) {
        std::fs::write(home.join(format!("rate-limit-{route}")), "hold: manual\nmessage: quota exhausted\n").expect("hold");
    }

    fn resolved_model(child: &mut RunArgs) -> Option<String> {
        let kind = AgentKind::parse_str(&child.agent_name).expect("kind");
        let _served = crate::agent::model_validation::MockServedModelsGuard::set(kind, None);
        let store = std::sync::Arc::new(Store::open_memory().expect("store"));
        crate::cmd::run::run_dispatch_resolve::resolve_agent_setup(&store, child, None)
            .expect("agent setup").effective_model
    }

    #[test]
    fn plan_filters_unavailable_disabled_auth_failed_and_below_floor() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid, AgentKind::Qwen, AgentKind::Grok]);
        crate::agent_config::save_agent_disabled("grok", true).expect("disable");
        crate::auth_marker::record_failure_at(AgentKind::Droid, "Not signed in.", chrono::Local::now());
        // Rated Qwen (7.4) is below the complex floor (8); an unrated route stays eligible.
        crate::agent_config::save_agent_default_model("qwen", Some("coder-model")).expect("model");
        let declared = DeclaredTaskProfile { difficulty: TaskDifficulty::Complex, ..profile() };
        let all = advise("refactor the scheduler", declared,
            Some(TaskCategory::Refactoring), None, None, 0, None).candidates;
        let item = |agent: &str| all.iter().find(|c| c.agent == agent).expect("candidate");
        assert!(item("agy").exclusion_codes.contains(&"not_installed".into()));
        assert!(item("droid").exclusion_codes.contains(&"auth_failed".into()));
        assert!(item("qwen").exclusion_codes.contains(&"below_floor".into()));
        assert!(all.iter().all(|c| c.agent != "grok"));
        let plan = expand_best_of_plan(all, None, 5).expect("plan");
        assert!(plan.iter().all(|c| c.agent == "codex"));
    }

    #[test]
    fn plan_excludes_agent_and_selected_model_group_holds_even_in_background() {
        let (temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Droid, AgentKind::Codex]);
        crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
        for (route, excluded, expected) in [("codex", "codex", "droid"), ("droid--standard", "droid", "codex")] {
            hold(temp.path(), route);
            let declared = DeclaredTaskProfile { urgency: TaskUrgency::Background, ..profile() };
            let report = advise("refactor the scheduler", declared, Some(TaskCategory::Refactoring), None, None, 0, None);
            assert!(report.candidates.iter().any(|c| c.agent == excluded && c.quota.status == "held"));
            let plan = expand_best_of_plan(report.candidates, None, 3).expect("plan");
            assert!(plan.iter().all(|c| c.agent == expected));
            std::fs::remove_file(temp.path().join(format!("rate-limit-{route}"))).expect("clear");
        }
        hold(temp.path(), "droid--core");
        let plan = expand_best_of_plan(candidates(), None, 5).expect("other group is allowed");
        assert!(plan.iter().any(|c| c.agent == "droid" && c.model.as_deref() == Some("gpt-5.3-codex")));
    }

    #[test]
    fn plan_excludes_superseded_gemini_and_unpreferred_claude() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Gemini, AgentKind::Antigravity, AgentKind::Claude]);
        let report = advise("compare docs", profile(), Some(TaskCategory::Research), None, None, 0, None);
        assert!(report.candidates.iter().any(|c| c.agent == "gemini" && c.exclusion_codes.contains(&"superseded_by_agy".into())));
        let plan = expand_best_of_plan(report.candidates.clone(), None, 3).expect("plan");
        assert!(plan.iter().all(|c| c.agent == "agy"));
        let team: TeamConfig = toml::from_str("id = 'test'\ndisplay_name = 'Test'\npreferred_agents = ['claude']\n").expect("team");
        assert!(expand_best_of_plan(report.candidates, Some(&team), 3).expect("preferred").iter().any(|c| c.agent == "claude"));
    }

    #[test]
    fn plan_excludes_weaker_caller_pool_and_rejects_ineligible_recommendation_fallback() {
        let (temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid]);
        crate::agent_config::save_agent_default_model("codex", Some("gpt-5.6-sol")).expect("model");
        let mut caller = caller_advice("codex", Some("gpt-5.6-sol")).expect("caller");
        caller.capability = Some(99.0);
        let report = advise("refactor scheduler", profile(), Some(TaskCategory::Refactoring), None, None, 0, Some(caller));
        assert!(report.candidates.iter().any(|c| c.agent == "codex" && c.exclusion_codes.contains(&"weaker_on_caller_pool".into())));
        assert!(expand_best_of_plan(report.candidates, None, 3).expect("plan").iter().all(|c| c.agent == "droid"));
        hold(temp.path(), "codex");
        hold(temp.path(), "droid--standard");
        let report = advise("refactor scheduler", profile(), Some(TaskCategory::Refactoring), None, None, 0, None);
        assert!(report.recommended.is_some(), "held fallback remains advisory");
        let err = expand_best_of_plan(report.candidates, None, 3).expect_err("no launchable routes");
        assert_eq!(err.to_string(), "best-of-3: no launchable advise candidates");
    }

    #[test]
    fn plan_keeps_ranked_full_candidates_then_cycles_without_replacing_models() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid, AgentKind::Cursor]);
        crate::agent_config::save_agent_default_model("codex", Some("gpt-5.6-sol")).expect("model");
        crate::agent_config::save_agent_default_model("droid", Some("gpt-5.3-codex")).expect("model");
        let all: Vec<_> = candidates().into_iter().filter(|c| c.launchable(None)).collect();
        assert_eq!(all.len(), 3);
        assert_eq!(expand_best_of_plan(all.clone(), None, 2).expect("top two"), all[..2]);
        let plan = expand_best_of_plan(all.clone(), None, 5).expect("cycles");
        assert_eq!(plan, vec![all[0].clone(), all[1].clone(), all[2].clone(), all[0].clone(), all[1].clone()]);
        assert!(expand_best_of_plan(Vec::new(), None, 2).is_err());
    }

    #[test]
    fn racers_replace_same_agent_explicit_model_and_clear_only_cross_agent_sessions() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex, AgentKind::Droid]);
        for agent in ["codex", "droid"] {
            crate::agent_config::save_agent_default_model(agent, Some("gpt-5.3-codex")).expect("model");
        }
        let parent = RunArgs {
            agent_name: "codex".into(), model: Some("parent-model".into()), session_id: Some("parent-session".into()),
            budget: true, force_default_model: true, best_of: Some(2), judge: Some("codex".into()), announce: true,
            ..Default::default()
        };
        for candidate in candidates().iter().filter(|c| c.launchable(None)) {
            let mut child = racer_args(&parent, candidate);
            assert_eq!(child.model, candidate.model);
            assert_eq!(child.model_source, ModelSource::Advised);
            assert!(!child.force_default_model);
            assert_eq!(child.session_id.as_deref(), (candidate.agent == "codex").then_some("parent-session"));
            assert!(child.background && child.best_of.is_none() && child.judge.is_none() && !child.announce);
            assert_eq!(resolved_model(&mut child), candidate.model, "budget must not replace the selected model");
        }
    }

    #[test]
    fn racers_pin_known_cli_defaults_and_keep_unknown_defaults_under_budget_pressure() {
        let (temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
        let codex = temp.path().join("codex");
        std::fs::create_dir(&codex).expect("codex home");
        crate::agent::codex::cli_config::set_test_codex_home(Some(codex.clone()));
        for model in [Some("gpt-6-sol"), None] {
            let config = codex.join("config.toml");
            if let Some(model) = model {
                std::fs::write(&config, format!("model = '{model}'\n")).expect("config");
            } else {
                std::fs::remove_file(&config).expect("remove config");
            }
            let candidate = candidates().into_iter().find(|c| c.agent == "codex").expect("codex");
            assert!(!candidate.pinned);
            assert_eq!(candidate.source, if model.is_some() { RunModelSource::CliConfig } else { RunModelSource::AgentDefault });
            let parent = RunArgs { agent_name: "codex".into(), model: Some("parent-model".into()), budget: true, ..Default::default() };
            let mut child = racer_args(&parent, &candidate);
            assert_eq!(child.model.as_deref(), model);
            assert_eq!(child.force_default_model, model.is_none());
            std::fs::write(temp.path().join("config.toml"), "[selection]\nbudget_mode = true\n").expect("budget pressure");
            assert_eq!(resolved_model(&mut child).as_deref(), model);
            std::fs::remove_file(temp.path().join("config.toml")).expect("clear budget pressure");
        }
        crate::agent::codex::cli_config::set_test_codex_home(None);
    }

    #[test]
    fn budget_racers_keep_the_advised_pin_and_all_profile_inputs() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::OpenCode]);
        let store = Store::open_memory().expect("store");
        for budget in [TaskBudget::Free, TaskBudget::Cheap] {
            let parent = RunArgs {
                agent_name: "codex".into(), prompt: "rename a field".into(), kind: Some(TaskCategory::SimpleEdit),
                declared_difficulty: Some(TaskDifficulty::Simple), declared_budget: Some(budget),
                model: Some("parent-model".into()), session_id: Some("parent-session".into()),
                ..Default::default()
            };
            let (_, plan) = advised_plan(&store, &parent, 2).expect("budget plan");
            assert!(plan[0].pinned);
            assert_eq!(plan[0].source, RunModelSource::BudgetRoute);
            let mut child = racer_args(&parent, &plan[0]);
            assert!(child.session_id.is_none());
            assert_eq!(child.declared_budget, Some(budget));
            assert_eq!(resolved_model(&mut child), plan[0].model);
        }
    }

    #[test]
    fn advised_plan_materializes_all_default_and_declared_profile_inputs() {
        let (_temp, _home, _cache) = isolated();
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Codex]);
        let store = Store::open_memory().expect("store");
        for args in [RunArgs { prompt: "refactor scheduler".into(), kind: Some(TaskCategory::Refactoring), ..Default::default() }, RunArgs {
            declared_difficulty: Some(TaskDifficulty::Complex), declared_budget: Some(TaskBudget::Premium),
            declared_urgency: Some(TaskUrgency::Urgent), declared_rigor: Some(TaskRigor::Critical),
            kind: Some(TaskCategory::Refactoring), ..Default::default()
        }] {
            let (declared, plan) = advised_plan(&store, &args, 2).expect("plan");
            assert_eq!(declared.difficulty, args.declared_difficulty.unwrap_or_default());
            assert_eq!(declared.budget, args.declared_budget.unwrap_or_default());
            assert_eq!(declared.urgency, args.declared_urgency.unwrap_or_default());
            assert_eq!(declared.rigor, args.declared_rigor.unwrap_or_default());
            let child = racer_args(&args, &plan[0]);
            assert_eq!(child.declared_difficulty, Some(declared.difficulty));
            assert_eq!(child.declared_budget, Some(declared.budget));
            assert_eq!(child.declared_urgency, Some(declared.urgency));
            assert_eq!(child.declared_rigor, Some(declared.rigor));
        }
    }
}
