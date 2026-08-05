// Category- and health-aware auto-cascade fallback selection.
// Picks the best installed alternative via the capability matrix; skips dead paths.
// Deps: classifier, capabilities, scoring priority, detect_agents, rate_limit, agent_config.

use super::classifier::{self, TaskCategory};
use super::detect_agents;
use super::selection_capabilities::{base_score, AGENT_CAPABILITIES};
use super::selection_scoring::priority;
use crate::agent_config;
use crate::rate_limit;
use crate::types::AgentKind;

/// Resolve fallback from a prompt: classify category, then pick a healthy peer.
pub(crate) fn coding_fallback_for_prompt(
    agent: &AgentKind,
    prompt: &str,
) -> Option<AgentKind> {
    let category = category_from_prompt(prompt);
    coding_fallback_for_category(agent, category)
}

/// Resolve fallback using a stored/declared category label when available.
pub(crate) fn coding_fallback_for(
    agent: &AgentKind,
    category: Option<&str>,
    prompt: Option<&str>,
) -> Option<AgentKind> {
    let resolved = category
        .and_then(TaskCategory::parse_str)
        .or_else(|| prompt.map(category_from_prompt))
        .unwrap_or(TaskCategory::ComplexImpl);
    coding_fallback_for_category(agent, resolved)
}

pub(crate) fn coding_fallback_for_category(
    agent: &AgentKind,
    category: TaskCategory,
) -> Option<AgentKind> {
    let available = detect_agents();
    pick_fallback(agent, category, &available)
}

fn category_from_prompt(prompt: &str) -> TaskCategory {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = classifier::count_file_mentions(&normalized);
    classifier::classify(prompt, file_count, prompt_len).category
}

fn pick_fallback(
    exhausted: &AgentKind,
    category: TaskCategory,
    available: &[AgentKind],
) -> Option<AgentKind> {
    let mut best: Option<(AgentKind, i32, bool, i32)> = None;
    for &kind in available {
        if kind == *exhausted {
            continue;
        }
        if !is_usable_fallback(kind, available) {
            continue;
        }
        let score = base_score(kind, category);
        let specialist = is_category_specialist(kind, category, score);
        let prio = priority(kind);
        let replace = match best {
            None => true,
            Some((_, best_score, best_spec, best_prio)) => {
                score > best_score
                    || (score == best_score && specialist && !best_spec)
                    || (score == best_score && specialist == best_spec && prio > best_prio)
            }
        };
        if replace {
            best = Some((kind, score, specialist, prio));
        }
    }
    best.map(|(kind, _, _, _)| kind)
}

/// True when `score` is this agent's peak across the capability matrix.
fn is_category_specialist(kind: AgentKind, category: TaskCategory, score: i32) -> bool {
    let Some((_, caps)) = AGENT_CAPABILITIES.iter().find(|(k, _)| *k == kind) else {
        return false;
    };
    caps.iter().any(|(cat, s)| *cat == category && *s == score)
        && caps.iter().all(|(_, s)| *s <= score)
}

fn is_usable_fallback(kind: AgentKind, available: &[AgentKind]) -> bool {
    !agent_config::is_agent_disabled(kind.as_str())
        && !rate_limit::is_rate_limited(&kind)
        && !is_known_unhealthy(kind, available)
}

/// Permanent dead paths that must not receive cascade work.
/// Gemini individual tier is superseded by Antigravity when agy is installed.
fn is_known_unhealthy(kind: AgentKind, available: &[AgentKind]) -> bool {
    matches!(kind, AgentKind::Gemini) && available.contains(&AgentKind::Antigravity)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::agent::DetectAgentsGuard;
    use crate::paths::AidHomeGuard;

    fn isolated() -> (tempfile::TempDir, AidHomeGuard) {
        let dir = tempfile::tempdir().expect("tempdir");
        let guard = AidHomeGuard::set(dir.path());
        std::fs::create_dir_all(crate::paths::aid_dir()).expect("aid dir");
        (dir, guard)
    }

    #[test]
    fn frontend_cascade_from_codex_picks_cursor_not_gemini() {
        let (_temp, _guard) = isolated();
        let _agents = DetectAgentsGuard::set(vec![
            AgentKind::Gemini,
            AgentKind::Antigravity,
            AgentKind::Codex,
            AgentKind::Cursor,
            AgentKind::OpenCode,
            AgentKind::Droid,
            AgentKind::Claude,
        ]);
        let got = coding_fallback_for_prompt(
            &AgentKind::Codex,
            "Create responsive React component for the settings page",
        );
        assert_eq!(got, Some(AgentKind::Cursor));
    }

    #[test]
    fn research_cascade_from_codex_picks_agy_not_gemini() {
        let (_temp, _guard) = isolated();
        let _agents = DetectAgentsGuard::set(vec![
            AgentKind::Gemini,
            AgentKind::Antigravity,
            AgentKind::Codex,
            AgentKind::Qwen,
            AgentKind::OpenCode,
            AgentKind::Claude,
        ]);
        let got = coding_fallback_for_prompt(
            &AgentKind::Codex,
            "Explain the authentication flow and compare the docs?",
        );
        assert_eq!(got, Some(AgentKind::Antigravity));
        assert_ne!(got, Some(AgentKind::Gemini));
    }

    #[test]
    fn complex_impl_skips_rate_limited_and_unhealthy_gemini() {
        let (_temp, _guard) = isolated();
        let _agents = DetectAgentsGuard::set(vec![
            AgentKind::Gemini,
            AgentKind::Antigravity,
            AgentKind::Codex,
            AgentKind::Droid,
            AgentKind::OpenCode,
            AgentKind::Cursor,
        ]);
        rate_limit::mark_rate_limited(&AgentKind::Droid, "quota exhausted");
        let got = coding_fallback_for_category(&AgentKind::Codex, TaskCategory::ComplexImpl);
        assert_eq!(got, Some(AgentKind::Cursor));
        assert_ne!(got, Some(AgentKind::Gemini));
        assert_ne!(got, Some(AgentKind::OpenCode));
        assert_ne!(got, Some(AgentKind::Droid));
    }

    #[test]
    fn skips_disabled_candidate_and_still_returns_usable() {
        let (_temp, _guard) = isolated();
        let _agents = DetectAgentsGuard::set(vec![
            AgentKind::Codex,
            AgentKind::Cursor,
            AgentKind::Droid,
        ]);
        agent_config::save_agent_disabled("droid", true).expect("disable");
        let got = coding_fallback_for_category(&AgentKind::Codex, TaskCategory::ComplexImpl);
        assert_eq!(got, Some(AgentKind::Cursor));
    }

    #[test]
    fn returns_none_only_when_no_usable_peer_exists() {
        let (_temp, _guard) = isolated();
        let _agents = DetectAgentsGuard::set(vec![AgentKind::MiMoCode]);
        assert_eq!(
            coding_fallback_for_category(&AgentKind::MiMoCode, TaskCategory::ComplexImpl),
            None
        );
    }
}
