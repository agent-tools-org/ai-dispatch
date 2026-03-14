// Prompt classification for intelligent task routing.
// Categorizes prompts by type and estimates complexity.
// Exports: TaskCategory, Complexity, TaskProfile, classify()

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TaskCategory {
    Research,
    SimpleEdit,
    ComplexImpl,
    Frontend,
    Debugging,
    Testing,
    Refactoring,
    Documentation,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Complexity {
    Low,
    Medium,
    High,
}

pub struct TaskProfile {
    pub category: TaskCategory,
    pub complexity: Complexity,
}

impl TaskCategory {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Research => "research",
            Self::SimpleEdit => "simple-edit",
            Self::ComplexImpl => "complex-impl",
            Self::Frontend => "frontend",
            Self::Debugging => "debugging",
            Self::Testing => "testing",
            Self::Refactoring => "refactoring",
            Self::Documentation => "documentation",
        }
    }
}

impl Complexity {
    pub fn label(&self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
        }
    }
}

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
const COMPLEX_IMPL_TERMS: &[&str] = &["implement", "create", "build"];
const DEBUGGING_TERMS: &[&str] = &[
    "debug",
    "fix bug",
    "investigate",
    "error",
    "crash",
    "panic",
    "trace",
    "root cause",
];
const TESTING_TERMS: &[&str] = &[
    "test",
    "spec",
    "coverage",
    "assertion",
    "mock",
    "fixture",
    "benchmark",
];
const REFACTORING_TERMS: &[&str] = &[
    "refactor",
    "restructure",
    "extract",
    "split",
    "reorganize",
    "decouple",
    "modularize",
];
const DOCUMENTATION_TERMS: &[&str] = &[
    "document",
    "readme",
    "changelog",
    "comment",
    "docstring",
    "api doc",
    "jsdoc",
];

pub(crate) const LOW_VALUE_TERMS: &[&str] = &[
    "run test",
    "cargo test",
    "cargo fmt",
    "cargo clippy",
    "format code",
    "lint",
    "update docs",
    "update readme",
    "update changelog",
    "add comment",
    "add docstring",
    "type annotation",
];
const FILE_SUFFIXES: &[&str] = &[
    ".rs", ".toml", ".md", ".json", ".yaml", ".yml", ".ts", ".tsx", ".js", ".jsx", ".css", ".html",
];

pub fn classify(prompt: &str, file_count: usize, prompt_len: usize) -> TaskProfile {
    let norm = prompt.trim().to_lowercase();

    let category = if contains_any_word(&norm, FRONTEND_TERMS) {
        TaskCategory::Frontend
    } else if RESEARCH_PREFIXES.iter().any(|p| norm.starts_with(p))
        || contains_any(&norm, RESEARCH_TERMS)
    {
        TaskCategory::Research
    } else if contains_any(&norm, SIMPLE_EDIT_TERMS) {
        TaskCategory::SimpleEdit
    } else if contains_any(&norm, COMPLEX_IMPL_TERMS) {
        TaskCategory::ComplexImpl
    } else if contains_any(&norm, TESTING_TERMS) {
        TaskCategory::Testing
    } else if contains_any(&norm, DEBUGGING_TERMS) {
        TaskCategory::Debugging
    } else if contains_any(&norm, DOCUMENTATION_TERMS) {
        TaskCategory::Documentation
    } else if contains_any(&norm, REFACTORING_TERMS) {
        TaskCategory::Refactoring
    } else if file_count > 0 {
        TaskCategory::ComplexImpl
    } else {
        TaskCategory::Research
    };

    let has_scope = contains_any(&norm, &["across", "all files", "entire"]);
    let complexity = if prompt_len > 500 || file_count > 3 || has_scope {
        Complexity::High
    } else if prompt_len < 150 && file_count <= 1 {
        Complexity::Low
    } else {
        Complexity::Medium
    };

    TaskProfile {
        category,
        complexity,
    }
}

pub(crate) fn count_file_mentions(prompt: &str) -> usize {
    prompt
        .split_whitespace()
        .map(trim_token)
        .filter(|tok| tok.contains('/') || FILE_SUFFIXES.iter().any(|s| tok.ends_with(s)))
        .count()
}

fn trim_token(token: &str) -> &str {
    token.trim_matches(|ch: char| !ch.is_alphanumeric() && ch != '.' && ch != '_' && ch != '/')
}

pub(crate) fn contains_any(prompt: &str, terms: &[&str]) -> bool {
    terms.iter().any(|term| prompt.contains(term))
}

/// Word-boundary aware match: "ui" matches " ui " but not "suite".
fn contains_any_word(text: &str, terms: &[&str]) -> bool {
    let bytes = text.as_bytes();
    terms.iter().any(|term| {
        text.match_indices(term).any(|(i, _)| {
            let before = i == 0 || !bytes[i - 1].is_ascii_alphanumeric();
            let end = i + term.len();
            let after = end >= bytes.len() || !bytes[end].is_ascii_alphanumeric();
            before && after
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn research_question() {
        let p = classify("Explain the authentication flow?", 0, 35);
        assert_eq!(p.category, TaskCategory::Research);
    }

    #[test]
    fn simple_edit_rename() {
        let p = classify("rename field in types.rs", 1, 24);
        assert_eq!(p.category, TaskCategory::SimpleEdit);
    }

    #[test]
    fn frontend_react() {
        let p = classify("Create responsive React component", 0, 34);
        assert_eq!(p.category, TaskCategory::Frontend);
    }

    #[test]
    fn complex_impl_long() {
        let prompt = "Implement a multi-file feature across many modules. ".repeat(12);
        let p = classify(&prompt, 5, prompt.len());
        assert_eq!(p.category, TaskCategory::ComplexImpl);
        assert_eq!(p.complexity, Complexity::High);
    }

    #[test]
    fn debugging_category() {
        let p = classify("debug the panic in parser", 0, 25);
        assert_eq!(p.category, TaskCategory::Debugging);
    }

    #[test]
    fn testing_category() {
        let p = classify("add unit tests for auth module", 0, 30);
        assert_eq!(p.category, TaskCategory::Testing);
    }

    #[test]
    fn refactoring_category() {
        let p = classify("refactor the dispatch module", 0, 28);
        assert_eq!(p.category, TaskCategory::Refactoring);
    }

    #[test]
    fn low_complexity_short() {
        let p = classify("fix typo in name", 0, 16);
        assert_eq!(p.complexity, Complexity::Low);
    }

    #[test]
    fn high_complexity_long() {
        let prompt = "x".repeat(600);
        let p = classify(&prompt, 5, 600);
        assert_eq!(p.complexity, Complexity::High);
    }
}
