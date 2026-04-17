// Advisory hints for explicit `aid run <agent>` dispatches.
// Exports non-blocking recommendation emission and hint flag text.
// Deps: agent selection, CLI flag constants, store/team context.

use crate::agent::{self, RunOpts};
use crate::cli::command_args_a::NO_HINT_FLAG;
use crate::store::Store;
use crate::team::TeamConfig;
use crate::types::AgentKind;

const MIN_HINT_PROMPT_CHARS: usize = 20;

pub(super) fn emit_if_recommended(
    user_agent: &str,
    prompt: &str,
    no_hint: bool,
    opts: &RunOpts,
    store: &Store,
    team: Option<&TeamConfig>,
) {
    if hint_suppressed(user_agent, prompt, no_hint) {
        return;
    }
    let (recommended, _) = agent::select_agent_with_reason(prompt, opts, store, team);
    if let Some(hint) = recommendation_hint(user_agent, prompt, no_hint, &recommended) {
        aid_hint!("{hint}");
    }
}

fn recommendation_hint(
    user_agent: &str,
    prompt: &str,
    no_hint: bool,
    recommended_agent: &str,
) -> Option<String> {
    if hint_suppressed(user_agent, prompt, no_hint) {
        return None;
    }
    if user_agent.eq_ignore_ascii_case(recommended_agent) {
        return None;
    }

    let detail = match AgentKind::parse_str(recommended_agent) {
        Some(AgentKind::Codex) => return None,
        Some(AgentKind::OpenCode) => " (5-20x cheaper, good for simple edits)",
        Some(AgentKind::Gemini) => " (subscription-based, good for research/docs/web queries)",
        Some(AgentKind::Cursor) => " (subscription-based, good for UI/frontend)",
        _ => "",
    };
    Some(format!(
        "[tip] For this prompt, `{recommended_agent}` would likely work too{detail}. Run with `aid run auto ...` next time to let aid choose. Pass --{NO_HINT_FLAG} to suppress."
    ))
}

fn hint_suppressed(user_agent: &str, prompt: &str, no_hint: bool) -> bool {
    no_hint || user_agent == "auto" || prompt.chars().count() < MIN_HINT_PROMPT_CHARS
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::selection::select_agent_from;
    use crate::paths::{self, AidHomeGuard};
    use tempfile::TempDir;

    fn opts() -> RunOpts {
        RunOpts {
            dir: None,
            output: None,
            result_file: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
            env: None,
            env_forward: None,
        }
    }

    fn isolated() -> (TempDir, AidHomeGuard) {
        let temp = TempDir::new().unwrap();
        let guard = AidHomeGuard::set(temp.path());
        std::fs::create_dir_all(paths::aid_dir()).unwrap();
        (temp, guard)
    }

    fn selected_for(prompt: &str) -> String {
        let (_temp, _guard) = isolated();
        let store = Store::open_memory().unwrap();
        let available = [
            AgentKind::Gemini,
            AgentKind::Qwen,
            AgentKind::Claude,
            AgentKind::OpenCode,
            AgentKind::Kilo,
            AgentKind::Cursor,
            AgentKind::Codex,
        ];
        select_agent_from(prompt, &opts(), &available, &store, None).0
    }

    #[test]
    fn hint_fires_when_user_picks_codex_but_classifier_recommends_opencode() {
        let prompt = "rename src/types.rs field name to task_name";
        let recommended = selected_for(prompt);

        assert_eq!(recommended, AgentKind::OpenCode.as_str());
        assert_eq!(
            recommendation_hint("codex", prompt, false, &recommended),
            Some("[tip] For this prompt, `opencode` would likely work too (5-20x cheaper, good for simple edits). Run with `aid run auto ...` next time to let aid choose. Pass --no-hint to suppress.".to_string())
        );
    }

    #[test]
    fn hint_suppressed_with_no_hint_flag() {
        assert_eq!(
            recommendation_hint("codex", "rename src/types.rs field name", true, "opencode"),
            None
        );
    }

    #[test]
    fn hint_suppressed_for_short_prompts() {
        assert_eq!(recommendation_hint("codex", "rename field", false, "opencode"), None);
    }

    #[test]
    fn hint_suppressed_when_user_picked_auto() {
        assert_eq!(
            recommendation_hint("auto", "rename src/types.rs field name", false, "opencode"),
            None
        );
    }

    #[test]
    fn hint_suppressed_when_classifier_agrees_with_user_choice() {
        assert_eq!(
            recommendation_hint("opencode", "rename src/types.rs field name", false, "opencode"),
            None
        );
    }
}
