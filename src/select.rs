// Smart agent selection heuristics for aid run auto.
// Classifies prompts to pick the best agent by cost and capability.
// Exports: select_agent(); Deps: crate::types::AgentKind.

use crate::types::AgentKind;

const QUESTION_WORDS: &[&str] = &[
    "what",
    "how",
    "why",
    "when",
    "where",
    "which",
    "who",
    "is",
    "are",
    "does",
    "can",
    "explain",
    "describe",
    "list",
    "compare",
    "summarize",
];
const RESEARCH_TERMS: &[&str] = &[
    "research",
    "investigate",
    "find out",
    "look up",
    "search",
    "documentation",
    "docs",
];
const FRONTEND_TERMS: &[&str] = &[
    "react",
    "vue",
    "svelte",
    "angular",
    "css",
    "html",
    "tailwind",
    "styled-component",
    "jsx",
    "tsx",
    "frontend",
    "landing page",
    "responsive",
    "component",
    "layout",
    "navigation",
    "header",
    "footer",
    "sidebar",
    "modal",
    "button",
    "form",
    "ui",
];
const SIMPLE_EDIT_TERMS: &[&str] = &[
    "rename",
    "replace",
    "change type",
    "add field",
    "remove field",
    "update import",
    "fix typo",
    "add comment",
    "change name",
    "swap",
    "toggle",
    "flip",
];
const COMPLEX_TASK_TERMS: &[&str] = &[
    "implement",
    "create",
    "build",
    "design",
    "architect",
    "refactor",
];

pub fn select_agent(prompt: &str, has_worktree: bool) -> AgentKind {
    let (kind, reason) = classify(prompt, has_worktree);
    println!("[select] {reason}");
    kind
}

fn classify(prompt: &str, has_worktree: bool) -> (AgentKind, &'static str) {
    let normalized = prompt.trim().to_lowercase();

    if is_research(&normalized) {
        (AgentKind::Gemini, "research/question → gemini")
    } else if is_frontend(&normalized) {
        (AgentKind::Cursor, "frontend/UI → cursor")
    } else if is_simple_edit(&normalized) {
        (AgentKind::OpenCode, "simple edit → opencode")
    } else if has_worktree {
        (AgentKind::Codex, "worktree task → codex")
    } else {
        (AgentKind::Codex, "general task → codex")
    }
}

fn is_research(prompt: &str) -> bool {
    prompt.contains('?')
        || starts_with_question_word(prompt)
        || contains_any(prompt, RESEARCH_TERMS)
}

fn is_frontend(prompt: &str) -> bool {
    contains_any(prompt, FRONTEND_TERMS)
}

fn is_simple_edit(prompt: &str) -> bool {
    contains_any(prompt, SIMPLE_EDIT_TERMS) && !contains_any(prompt, COMPLEX_TASK_TERMS)
}

fn starts_with_question_word(prompt: &str) -> bool {
    prompt
        .split_whitespace()
        .next()
        .map(|word| word.trim_matches(|c: char| !c.is_alphabetic()))
        .is_some_and(|word| QUESTION_WORDS.contains(&word))
}

fn contains_any(prompt: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| prompt.contains(term))
}

#[cfg(test)]
mod tests {
    use super::select_agent;
    use crate::types::AgentKind;

    #[test]
    fn research_goes_to_gemini() {
        assert_eq!(
            select_agent("what does agent/mod.rs export?", false),
            AgentKind::Gemini,
        );
        assert_eq!(
            select_agent("research uniswap v4 hooks", false),
            AgentKind::Gemini,
        );
    }

    #[test]
    fn frontend_goes_to_cursor() {
        assert_eq!(
            select_agent("create a react landing page", false),
            AgentKind::Cursor,
        );
    }

    #[test]
    fn simple_edit_goes_to_opencode() {
        assert_eq!(
            select_agent("rename foo to bar in types.rs", false),
            AgentKind::OpenCode,
        );
    }

    #[test]
    fn worktree_goes_to_codex() {
        assert_eq!(
            select_agent("implement retry logic", true),
            AgentKind::Codex,
        );
    }

    #[test]
    fn default_goes_to_codex() {
        assert_eq!(
            select_agent("implement new store layer", false),
            AgentKind::Codex,
        );
    }
}
