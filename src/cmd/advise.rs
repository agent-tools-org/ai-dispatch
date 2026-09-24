// Human and JSON output for read-only declared-profile agent advice.
// Exports: run(), build_report().
// Deps: CLI args, selection advice payload, optional read-only store, teams.

use anyhow::Result;

use crate::agent::classifier::TaskCategory;
use crate::agent::selection::{AdviceReport, advise, caller_advice};
use crate::cli::command_args_advise::AdviseArgs;
use crate::store::Store;
use crate::types::DeclaredTaskProfile;

pub(crate) fn run(store: Option<&Store>, args: AdviseArgs) -> Result<()> {
    crate::live_quota_refresh::refresh_stale_if_enabled();
    let declared = DeclaredTaskProfile {
        difficulty: args.difficulty,
        budget: args.budget,
        urgency: args.urgency,
        rigor: args.rigor,
    };
    let kind_was_overridden = args.kind.is_some();
    let report = build_report(
        store,
        &args.prompt,
        declared,
        args.kind,
        args.team.as_deref(),
        args.top,
        args.caller_model.as_deref(),
    );
    if args.json {
        println!("{}", serde_json::to_string_pretty(&report)?);
    } else {
        print_human(&report, kind_was_overridden);
    }
    Ok(())
}

pub(crate) fn build_report(
    store: Option<&Store>,
    prompt: &str,
    declared: DeclaredTaskProfile,
    kind: Option<TaskCategory>,
    team: Option<&str>,
    top: usize,
    caller_model: Option<&str>,
) -> AdviceReport {
    let team = team.and_then(crate::team::resolve_team);
    let model = crate::session::caller_model(caller_model);
    let caller = crate::session::current_caller()
        .and_then(|session| caller_advice(&session.kind, model.as_deref()));
    advise(prompt, declared, kind, team.as_ref(), store, top, caller)
}

fn print_human(report: &AdviceReport, kind_was_overridden: bool) {
    let source = if kind_was_overridden { "declared" } else { "inferred" };
    println!(
        "Declared: {} / {} / {} / {}   (kind: {}, {})",
        report.declared.difficulty.label(),
        report.declared.budget.label(),
        report.declared.urgency.label(),
        report.declared.rigor.label(),
        report.inferred.kind.label(),
        source,
    );
    if let Some(recommended) = &report.recommended {
        // The recommendation names a route, not an agent id. `codex` alone
        // cannot say which quota pool the work will draw on, and that was the
        // question that mattered every time routing broke on 2026-08-05: an
        // exhausted route says nothing about a different provider that reaches
        // a model of the same class.
        let route = recommended_route(&recommended.agent, recommended.model.clone());
        println!(
            "Recommended: {}   score {:.1}   {}  {}{}",
            route,
            recommended.score,
            cost_label(recommended.est_cost_usd),
            duration_label(recommended.est_duration_secs),
            recommended_quota_suffix(&recommended.reason),
        );
    } else {
        println!("Recommended: none (no installed agents)");
    }
    if let Some(caller) = &report.caller {
        println!(
            "Caller: {} → {} pool (model {})",
            caller.session, caller.provider, caller.model.as_deref().unwrap_or("unknown"),
        );
    }
    for (index, candidate) in report.candidates.iter().enumerate() {
        let availability = candidate_mark(
            candidate.exclusion_reason.as_deref().or(candidate.demotion_reason.as_deref()),
        );
        let item = &candidate.breakdown;
        println!(
            "  {}. {:<10} {:>5.1}  base {:.1}  {:+.1} model  {:+.1} budget  {:+.1} limit  {:+.1} history  {:+.1} complexity  {:+.1} team  {:+.1} headroom{}",
            index + 1,
            candidate.agent,
            candidate.score,
            item.base,
            item.model_capability,
            item.budget_penalty,
            item.rate_limit_penalty,
            item.history_bonus,
            item.complexity_bonus,
            item.team_bonus,
            item.headroom_penalty,
            availability,
        );
        if let Some(line) = unrated_served_line(candidate) {
            println!("{line}");
        }
    }
    if !report.custom_candidates.is_empty() {
        println!("Custom agents (separate capability scale):");
        for candidate in &report.custom_candidates {
            let availability = candidate_mark(candidate.exclusion_reason.as_deref());
            let preference = if candidate.team_preferred { "  team preferred" } else { "" };
            println!(
                "  {:<20} capability {}  +{} strength{}{}",
                candidate.agent,
                candidate.category_capability,
                candidate.strength_bonus,
                preference,
                availability,
            );
        }
    }
    if !report.notes.is_empty() {
        println!("Notes: {}", report.notes.join("; "));
    }
}

fn recommended_quota_suffix(reason: &str) -> String {
    reason
        .split_once("; ")
        .filter(|(_, rest)| rest.contains("held →"))
        .map(|(_, rest)| format!("  quota: {rest}"))
        .unwrap_or_default()
}

fn candidate_mark(reason: Option<&str>) -> String {
    reason.map(|reason| format!("  [{reason}]")).unwrap_or_default()
}

fn cost_label(cost: Option<f64>) -> String {
    cost.map(|value| format!("~${value:.2}"))
        .unwrap_or_else(|| "cost unknown".to_string())
}

fn duration_label(seconds: Option<i64>) -> String {
    seconds.map(|value| format!("~{}m", (value + 30) / 60))
        .unwrap_or_else(|| "duration unknown".to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn advice_payload_round_trips_through_json() {
        let home = tempfile::tempdir().expect("temp aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        let args = AdviseArgs {
            prompt: "Refactor src/main.rs".to_string(),
            difficulty: crate::types::TaskDifficulty::Complex,
            budget: crate::types::TaskBudget::Premium,
            urgency: crate::types::TaskUrgency::Urgent,
            rigor: crate::types::TaskRigor::Critical,
            kind: None,
            team: None,
            top: 5,
            json: true,
            dir: None,
            caller_model: None,
        };
        let declared = DeclaredTaskProfile {
            difficulty: args.difficulty,
            budget: args.budget,
            urgency: args.urgency,
            rigor: args.rigor,
        };
        let report = advise(&args.prompt, declared, None, None, None, args.top, None);
        let encoded = serde_json::to_value(&report).expect("serialize advice");
        let decoded: AdviceReport = serde_json::from_value(encoded).expect("parse advice");
        assert_eq!(decoded, report);
    }

    /// `advise` reads the real `~/.aid/rate-limit-*` markers and
    /// `agent_config.toml`, so without an isolated home this asserts against
    /// whatever quota state the developer's machine happens to be in. It failed
    /// twice in five full-suite runs on 2026-08-06 — codex was marked limited
    /// until Aug 11 and codebuff carried `disabled = true`, which puts the
    /// eligible count right at the threshold — while passing in isolation. An
    /// empty AID_HOME gives every agent its unlimited default.
    #[test]
    fn complex_critical_surfaces_alternatives_with_shortfall_reasons() {
        let home = tempfile::tempdir().expect("temp aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        // Eligibility now requires an installed binary; pin the fleet.
        use crate::types::AgentKind;
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![
            AgentKind::Codex, AgentKind::Droid, AgentKind::Claude, AgentKind::Cursor,
        ]);
        let declared = DeclaredTaskProfile {
            difficulty: crate::types::TaskDifficulty::Complex,
            budget: crate::types::TaskBudget::Premium,
            urgency: crate::types::TaskUrgency::Normal,
            rigor: crate::types::TaskRigor::Critical,
        };
        let report = advise(
            "Refactor the scheduler across modules",
            declared,
            Some(crate::agent::classifier::TaskCategory::Refactoring),
            None,
            None,
            5,
            None,
        );
        assert!(report.recommended.is_some());
        let eligible = report.candidates.iter().filter(|c| c.eligible).count();
        assert!(eligible >= 2, "rigor must not collapse alternatives to one local agent");
        let cursor = report.candidates.iter().find(|c| c.agent == "cursor");
        if let Some(cursor) = cursor {
            let reason = cursor.exclusion_reason.as_deref().unwrap_or("");
            assert!(
                reason.contains("base 6 < floor 8 for complex"),
                "expected floor shortfall, got {reason:?}"
            );
        }
    }
}

/// `<cli>/<provider>/<model>` for a recommendation. A custom agent has no
/// builtin `AgentKind`, and its provider is genuinely unestablished rather than
/// absent — say `unknown` rather than dropping the dimension, so the caller can
/// see which part aid could not answer.
fn recommended_route(agent: &str, model: Option<String>) -> String {
    match crate::types::AgentKind::parse_str(agent) {
        Some(cli) if cli != crate::types::AgentKind::Custom => {
            crate::types::Route::for_cli(cli).with_model(model).id()
        }
        _ => format!("{agent}/unknown/{}", model.as_deref().unwrap_or("-")),
    }
}

#[cfg(test)]
mod recommended_route_tests {
    use super::recommended_route;

    #[test]
    fn a_builtin_agent_resolves_to_its_provider() {
        assert_eq!(
            recommended_route("codex", Some("gpt-5.6".to_string())),
            "codex/openai-chatgpt-plan/gpt-5.6"
        );
    }

    #[test]
    fn an_unpinned_model_is_shown_as_unpinned() {
        assert_eq!(recommended_route("cursor", None), "cursor/cursor-subscription/-");
    }

    /// A custom agent's provider is unestablished, not absent. Dropping the
    /// dimension would hide that aid does not know where the bill lands.
    #[test]
    fn a_custom_agent_keeps_the_dimension_and_admits_ignorance() {
        assert_eq!(recommended_route("glm5", Some("z-ai/glm5".to_string())), "glm5/unknown/z-ai/glm5");
    }
}

/// One line naming served models newer than the catalog pick; they stay unrated.
fn unrated_served_line(candidate: &crate::agent::selection::AdviceCandidate) -> Option<String> {
    let model = candidate.model.as_deref()?;
    (!candidate.unrated_served_models.is_empty()).then(|| format!(
        "     note: {} also serves newer unrated {} (catalog pick {model}; not auto-selected)",
        candidate.agent,
        candidate.unrated_served_models.join(", "),
    ))
}

#[cfg(test)]
mod unrated_served_tests {
    use super::*;

    #[test]
    fn advise_surfaces_newer_unrated_served_codex_model() {
        let home = tempfile::tempdir().expect("temp aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        crate::paths::ensure_dirs().expect("aid dirs");
        let now = chrono::Utc::now().timestamp();
        let cache = serde_json::json!({"codex": {"models": ["gpt-6-sol"], "updated_at_secs": now}});
        std::fs::write(crate::paths::aid_dir().join("served_models_cache.json"), cache.to_string())
            .expect("served-model cache");
        let declared = DeclaredTaskProfile {
            difficulty: crate::types::TaskDifficulty::Complex,
            budget: crate::types::TaskBudget::Premium,
            urgency: crate::types::TaskUrgency::Normal,
            rigor: crate::types::TaskRigor::Standard,
        };
        let report = advise("Implement the parser", declared, None, None, None, 20);
        let codex = report.candidates.iter().find(|c| c.agent == "codex").expect("codex");
        assert_eq!(codex.model.as_deref(), Some("gpt-5.6-sol"), "unrated model is not auto-selected");
        assert_eq!(codex.unrated_served_models, vec!["gpt-6-sol".to_string()]);
        let line = unrated_served_line(codex).expect("human line");
        assert!(line.contains("gpt-6-sol") && line.contains("gpt-5.6-sol"), "{line}");
    }
}
