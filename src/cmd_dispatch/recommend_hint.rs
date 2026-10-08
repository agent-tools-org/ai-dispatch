// Advisory hints for explicit `aid run <agent>` dispatches.
// Exports non-blocking recommendation emission and hint flag text.
// Deps: agent selection, CLI flag constants, store/team context.

use crate::agent::classifier::TaskCategory;
use crate::agent::selection::{advise, caller_advice};
use crate::cli::run_args::NO_HINT_FLAG;
use crate::store::Store;
use crate::team::TeamConfig;
use crate::types::{AgentKind, DeclaredTaskProfile};

const MIN_HINT_PROMPT_CHARS: usize = 20;

pub(super) fn recommendation_hint(
    user_agent: &str, prompt: &str, no_hint: bool, declared: DeclaredTaskProfile,
    kind: Option<TaskCategory>, store: &Store, team: Option<&TeamConfig>,
) -> Option<String> {
    if no_hint || prompt.chars().count() < MIN_HINT_PROMPT_CHARS {
        return None;
    }
    let model = crate::session::caller_model(None);
    let caller = crate::session::current_caller()
        .and_then(|session| caller_advice(&session.kind, model.as_deref(), kind.unwrap_or_else(|| {
            let normalized = prompt.trim().to_lowercase();
            crate::agent::classifier::classify(prompt,
                crate::agent::classifier::count_file_mentions(&normalized), prompt.chars().count()).category
        })));
    let recommended_agent = advise(prompt, declared, kind, team, Some(store), 0, caller)
        .recommended?.agent;
    if user_agent.eq_ignore_ascii_case(&recommended_agent) {
        return None;
    }

    let detail = match AgentKind::parse_str(&recommended_agent) {
        Some(AgentKind::Codex) => return None,
        Some(AgentKind::OpenCode) => " (5-20x cheaper, good for simple edits)",
        Some(AgentKind::Gemini) => " (subscription-based, good for research/docs/web queries)",
        Some(AgentKind::Cursor) => " (subscription-based, good for UI/frontend)",
        _ => "",
    };
    Some(format!(
        "[tip] For this prompt, `{recommended_agent}` would likely work too{detail}. Run `aid advise` to compare agents, then `aid run <agent> ...`. Pass --{NO_HINT_FLAG} to suppress."
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths::AidHomeGuard;
    use crate::types::{TaskBudget, TaskDifficulty, TaskRigor, TaskUrgency};
    use tempfile::TempDir;

    fn profile() -> DeclaredTaskProfile {
        DeclaredTaskProfile {
            difficulty: TaskDifficulty::Moderate, budget: TaskBudget::Standard,
            urgency: TaskUrgency::Normal, rigor: TaskRigor::Standard,
        }
    }

    fn hint(user: &str, prompt: &str, no_hint: bool) -> Option<String> {
        let temp = TempDir::new().expect("home");
        let _home = AidHomeGuard::set(temp.path());
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::OpenCode]);
        recommendation_hint(user, prompt, no_hint, profile(), None, &Store::open_memory().expect("store"), None)
    }

    #[test]
    fn hint_fires_when_user_picks_codex_but_advise_recommends_opencode() {
        let prompt = "rename src/types.rs field name to task_name";
        assert_eq!(
            hint("codex", prompt, false),
            Some("[tip] For this prompt, `opencode` would likely work too (5-20x cheaper, good for simple edits). Run `aid advise` to compare agents, then `aid run <agent> ...`. Pass --no-hint to suppress.".to_string())
        );
    }

    #[test]
    fn hint_suppressed_with_no_hint_flag() {
        assert_eq!(hint("codex", "rename src/types.rs field name", true), None);
    }

    #[test]
    fn hint_suppressed_for_short_prompts() {
        assert_eq!(hint("codex", "rename field", false), None);
    }

    #[test]
    fn hint_suppressed_when_advise_agrees_with_user_choice() {
        assert_eq!(hint("OPENCODE", "rename src/types.rs field name", false), None);
    }

    #[test]
    fn only_claude_installed_without_preference_keeps_hint_silent() {
        let temp = TempDir::new().expect("home");
        let _home = AidHomeGuard::set(temp.path());
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Claude]);
        let store = Store::open_memory().expect("store");
        assert_eq!(recommendation_hint("codex", "refactor the scheduler", false,
            profile(), None, &store, None), None);
    }

    #[test]
    fn hint_recommends_the_advise_agent_for_the_same_profile() {
        let temp = TempDir::new().expect("home");
        let _home = AidHomeGuard::set(temp.path());
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![AgentKind::Gemini, AgentKind::Droid, AgentKind::OpenCode]);
        let store = Store::open_memory().expect("store");
        let prompt = "refactor the scheduler and rename the routing fields";
        for (declared, kind) in [
            (DeclaredTaskProfile { difficulty: TaskDifficulty::Simple, budget: TaskBudget::Free,
                urgency: TaskUrgency::Background, rigor: TaskRigor::Draft }, TaskCategory::Research),
            (DeclaredTaskProfile { difficulty: TaskDifficulty::Complex, budget: TaskBudget::Premium,
                urgency: TaskUrgency::Urgent, rigor: TaskRigor::Critical }, TaskCategory::Refactoring),
        ] {
            let report = crate::cmd::advise::build_report(Some(&store), prompt, declared, Some(kind), None, 0, None);
            let recommended = report.recommended.expect("recommendation");
            let hint = recommendation_hint("qwen", prompt, false, declared, Some(kind), &store, None).expect("hint");
            assert!(hint.contains(&format!("`{}`", recommended.agent)), "{hint}");
        }
    }

}
