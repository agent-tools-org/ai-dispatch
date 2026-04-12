// Agent kind enum plus capability/profile metadata.
// Exports: AgentKind and its helper methods.
// Deps: serde and std::fmt.

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AgentKind {
    Gemini,
    Qwen,
    Codex,
    Copilot,
    OpenCode,
    Cursor,
    Kilo,
    Codebuff,
    Droid,
    Oz,
    Claude,
    Custom,
}

impl AgentKind {
    pub const ALL_BUILTIN: &'static [Self] = &[
        Self::Gemini,
        Self::Qwen,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::Cursor,
        Self::Kilo,
        Self::Codebuff,
        Self::Droid,
        Self::Oz,
        Self::Claude,
    ];

    pub const ALL: &'static [Self] = &[
        Self::Gemini,
        Self::Qwen,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::Cursor,
        Self::Kilo,
        Self::Codebuff,
        Self::Droid,
        Self::Oz,
        Self::Claude,
        Self::Custom,
    ];

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "gemini" => Some(Self::Gemini),
            "qwen" => Some(Self::Qwen),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            "opencode" => Some(Self::OpenCode),
            "cursor" => Some(Self::Cursor),
            "kilo" => Some(Self::Kilo),
            "codebuff" => Some(Self::Codebuff),
            "droid" => Some(Self::Droid),
            "oz" => Some(Self::Oz),
            "claude" => Some(Self::Claude),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Gemini => "gemini",
            Self::Qwen => "qwen",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::OpenCode => "opencode",
            Self::Cursor => "cursor",
            Self::Kilo => "kilo",
            Self::Codebuff => "codebuff",
            Self::Droid => "droid",
            Self::Oz => "oz",
            Self::Claude => "claude",
            Self::Custom => "custom",
        }
    }

    pub fn sandboxed_fs(&self) -> bool {
        matches!(self, Self::OpenCode)
    }

    pub fn profile(
        &self,
    ) -> Option<(&'static str, &'static str, &'static str, &'static str, bool, &'static str)> {
        match self {
            Self::Gemini => Some((
                "gemini",
                "Research, coding, web search, file editing",
                "$0.10-$10/M blended",
                "research, explain, implement, create, analyze, build",
                true,
                "api",
            )),
            Self::Qwen => Some((
                "qwen",
                "Research, coding with Qwen3-Coder models",
                "free (OAuth) or Alibaba Cloud subscription",
                "implement, refactor, research, explain",
                true,
                "api",
            )),
            Self::Codex => Some((
                "codex",
                "Complex implementation, multi-file refactors",
                "$0.10-$8/M blended",
                "implement, create, build, refactor, test",
                true,
                "local",
            )),
            Self::Copilot => Some((
                "copilot",
                "General coding, repo navigation, tool-assisted implementation",
                "subscription",
                "implement, build, refactor, test, explain, debug",
                true,
                "api",
            )),
            Self::OpenCode => Some((
                "opencode",
                "Simple edits, renames, type annotations",
                "free-$2/M blended",
                "rename, change, update, fix typo, add type",
                true,
                "api",
            )),
            Self::Cursor => Some((
                "cursor",
                "General coding, strong model selection, frontend",
                "$20/mo subscription",
                "implement, create, build, refactor, ui, frontend, css",
                true,
                "api",
            )),
            Self::Kilo => Some((
                "kilo",
                "Simple edits (free tier)",
                "free",
                "rename, change, update, fix typo, add type",
                true,
                "api",
            )),
            Self::Codebuff => Some((
                "aid-codebuff",
                "Complex implementation, frontend",
                "SDK-managed",
                "complex coding, frontend",
                true,
                "local",
            )),
            Self::Droid => Some((
                "droid",
                "Complex implementation, multi-agent orchestration",
                "BYOK (API key)",
                "implement, create, build, refactor, test, orchestrate",
                true,
                "api",
            )),
            Self::Oz => Some((
                "oz",
                "Complex implementation, multi-file refactors",
                "Warp subscription",
                "implement, create, build, refactor, test",
                true,
                "local",
            )),
            Self::Claude => Some((
                "claude",
                "General coding, review, refactoring, research",
                "$1-$75/M blended",
                "implement, review, refactor, explain, research, test",
                true,
                "api",
            )),
            Self::Custom => None,
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
