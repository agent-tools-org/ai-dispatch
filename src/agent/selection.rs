// Auto-selection heuristics for `aid run auto`.
// Scores prompt signals, respects installed CLIs, and returns a concise reason.
// Exports select_agent() helpers; deps: super::detect_agents, super::RunOpts.

use super::{detect_agents, RunOpts};
use crate::rate_limit;
use crate::types::AgentKind;

const RESEARCH_PREFIXES: &[&str] = &[
    "research:",
    "what is",
    "how does",
    "explain",
    "find",
    "list",
];
const RESEARCH_TERMS: &[&str] = &["?", "documentation", "compare", "analyze"];
const SIMPLE_EDIT_TERMS: &[&str] = &[
    "rename",
    "change",
    "update",
    "fix typo",
    "add type",
    "annotation",
];
const FRONTEND_TERMS: &[&str] = &[
    "ui",
    "frontend",
    "css",
    "html",
    "react",
    "component",
    "layout",
    "design",
    "responsive",
];
const COMPLEX_TERMS: &[&str] = &["implement", "create", "build", "refactor", "test suite"];
const FILE_SUFFIXES: &[&str] = &[
    ".rs", ".toml", ".md", ".json", ".yaml", ".yml", ".ts", ".tsx", ".js", ".jsx", ".css", ".html",
];

#[derive(Clone, Copy)]
struct Candidate {
    kind: AgentKind,
    score: i32,
    reason: &'static str,
}

pub(crate) fn select_agent_with_reason(prompt: &str, opts: &RunOpts) -> (AgentKind, String) {
    select_agent_from(prompt, opts, &detect_agents())
}

fn select_agent_from(prompt: &str, opts: &RunOpts, available: &[AgentKind]) -> (AgentKind, String) {
    let normalized = prompt.trim().to_lowercase();
    let prompt_len = prompt.chars().count();
    let file_count = count_file_mentions(&normalized);
    let has_workspace = opts.dir.is_some();
    let budget = opts.budget;
    let candidates = [
        Candidate {
            kind: AgentKind::Gemini,
            score: score_gemini(&normalized, has_workspace),
            reason: "research/question task",
        },
        Candidate {
            kind: AgentKind::OpenCode,
            score: score_opencode(&normalized, file_count, prompt_len, budget),
            reason: "simple edit task",
        },
        Candidate {
            kind: AgentKind::Cursor,
            score: score_cursor(&normalized),
            reason: "frontend/UI task",
        },
        Candidate {
            kind: AgentKind::Codex,
            score: score_codex(&normalized, file_count, prompt_len, budget),
            reason: codex_reason(&normalized, file_count, prompt_len),
        },
    ];
    let primary = best_candidate(&candidates, None);
    let selected = if available.is_empty() {
        primary
    } else {
        best_candidate(&candidates, Some(available))
    };
    let mut reason = if selected.kind == primary.kind {
        selected.reason.to_string()
    } else {
        format!("{}; {} unavailable", primary.reason, primary.kind.as_str())
    };
    if budget {
        reason.push_str("; budget mode: preferring cheaper agent");
    }
    if rate_limit::is_rate_limited(&AgentKind::Codex) && selected.kind != AgentKind::Codex {
        reason.push_str("; codex rate-limited");
    }
    (selected.kind, reason)
}

fn best_candidate(candidates: &[Candidate], available: Option<&[AgentKind]>) -> Candidate {
    candidates
        .iter()
        .copied()
        .filter(|candidate| match available {
            Some(items) => items.contains(&candidate.kind),
            None => true,
        })
        .max_by_key(|candidate| (candidate.score, priority(candidate.kind)))
        .unwrap_or(Candidate {
            kind: AgentKind::Codex,
            score: 1,
            reason: "general coding task",
        })
}

fn score_gemini(prompt: &str, has_workspace: bool) -> i32 {
    let starts_like_research = RESEARCH_PREFIXES
        .iter()
        .any(|term| prompt.starts_with(term));
    let has_research_terms = contains_any(prompt, RESEARCH_TERMS);
    let mut score = 0;
    if starts_like_research {
        score += 4;
    }
    if has_research_terms {
        score += 3;
    }
    if score > 0 && !has_workspace {
        score += 2;
    }
    score
}

fn score_opencode(prompt: &str, file_count: usize, prompt_len: usize, budget: bool) -> i32 {
    let mut score = 0;
    if contains_any(prompt, SIMPLE_EDIT_TERMS) {
        score += 4;
        if file_count == 1 {
            score += 2;
        }
        if prompt_len < 200 {
            score += 2;
        }
    }
    if budget {
        score += 4;
    }
    score
}

fn score_cursor(prompt: &str) -> i32 {
    if contains_any(prompt, FRONTEND_TERMS) {
        7
    } else {
        0
    }
}

fn score_codex(prompt: &str, file_count: usize, prompt_len: usize, budget: bool) -> i32 {
    let mut score = 1;
    if contains_any(prompt, COMPLEX_TERMS) {
        score += 4;
    }
    if prompt_len > 500 {
        score += 2;
    }
    if file_count > 1 || prompt.contains(" modules") || prompt.contains(" files") {
        score += 2;
    }
    if budget {
        score = (score - 8).max(0);
    }
    if rate_limit::is_rate_limited(&AgentKind::Codex) {
        score = (score - 10).max(0);
    }
    score
}

fn codex_reason(prompt: &str, file_count: usize, prompt_len: usize) -> &'static str {
    if contains_any(prompt, COMPLEX_TERMS) || prompt_len > 500 || file_count > 1 {
        "complex implementation task"
    } else {
        "general coding task"
    }
}

fn count_file_mentions(prompt: &str) -> usize {
    prompt
        .split_whitespace()
        .map(trim_token)
        .filter(|token| {
            token.contains('/') || FILE_SUFFIXES.iter().any(|suffix| token.ends_with(suffix))
        })
        .count()
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '.' && ch != '_' && ch != '/')
}

fn contains_any(prompt: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| prompt.contains(term))
}

fn priority(kind: AgentKind) -> i32 {
    match kind {
        AgentKind::Gemini => 0,
        AgentKind::OpenCode => 1,
        AgentKind::Cursor => 2,
        AgentKind::Codex => 3,
    }
}

#[cfg(test)]
mod tests {
    use super::select_agent_from;
    use crate::agent::RunOpts;
    use crate::types::AgentKind;

    #[test]
    fn research_tasks_go_to_gemini() {
        let (kind, reason) = select(
            "Explain the authentication flow and compare the docs?",
            &[],
            &available_agents(),
        );
        assert_eq!(kind, AgentKind::Gemini);
        assert_eq!(reason, "research/question task");
    }

    #[test]
    fn simple_edits_go_to_opencode() {
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name",
            &[],
            &available_agents(),
        );
        assert_eq!(kind, AgentKind::OpenCode);
        assert_eq!(reason, "simple edit task");
    }

    #[test]
    fn frontend_tasks_go_to_cursor() {
        let (kind, reason) = select(
            "Create a responsive React component layout for the settings UI",
            &["web/app.tsx"],
            &available_agents(),
        );
        assert_eq!(kind, AgentKind::Cursor);
        assert_eq!(reason, "frontend/UI task");
    }

    #[test]
    fn complex_tasks_go_to_codex() {
        let prompt = format!(
            "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. {}",
            "Add validation coverage and refactor the task dispatch flow. ".repeat(12)
        );
        let (kind, reason) = select(&prompt, &["src"], &available_agents());
        assert_eq!(kind, AgentKind::Codex);
        assert_eq!(reason, "complex implementation task");
    }

    #[test]
    fn unavailable_primary_agent_falls_back_to_next_best() {
        let (kind, reason) = select(
            "rename src/types.rs field name to task_name",
            &[],
            &[AgentKind::Gemini, AgentKind::Codex],
        );
        assert_eq!(kind, AgentKind::Codex);
        assert_eq!(reason, "simple edit task; opencode unavailable");
    }

    #[test]
    fn budget_mode_avoids_codex_for_complex_tasks() {
        let prompt =
            "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
        let opts = RunOpts {
            dir: Some("src".to_string()),
            output: None,
            model: None,
            budget: true,
            read_only: false,
            context_files: vec![],
            session_id: None,
        };
        let (kind, reason) = select_agent_from(prompt, &opts, &available_agents());
        assert_ne!(kind, AgentKind::Codex);
        assert!(reason.contains("budget"));
    }

    fn select(prompt: &str, dir: &[&str], available: &[AgentKind]) -> (AgentKind, String) {
        let opts = RunOpts {
            dir: dir.first().map(|value| value.to_string()),
            output: None,
            model: None,
            budget: false,
            read_only: false,
            context_files: vec![],
            session_id: None,
        };
        select_agent_from(prompt, &opts, available)
    }

    fn available_agents() -> [AgentKind; 4] {
        [
            AgentKind::Gemini,
            AgentKind::OpenCode,
            AgentKind::Cursor,
            AgentKind::Codex,
        ]
    }
}
