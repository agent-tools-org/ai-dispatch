// Agent kind enum plus capability/profile metadata.
// Exports: AgentKind and its helper methods.
// Deps: serde and std::fmt.

mod profiles;
use profiles::*;

use serde::Serialize;
use std::fmt;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
pub enum AgentKind {
    Qwen,
    Codex,
    Copilot,
    OpenCode,
    CommandCode,
    Cursor,
    Kilo,
    MiMoCode,
    Droid,
    Oz,
    Claude,
    Antigravity,
    Grok,
    Custom,
}

impl AgentKind {
    pub const ALL_BUILTIN: &'static [Self] = &[
        Self::Qwen,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::CommandCode,
        Self::Cursor,
        Self::Kilo,
        Self::MiMoCode,
        Self::Droid,
        Self::Oz,
        Self::Antigravity,
        Self::Grok,
    ];

    pub const ALL: &'static [Self] = &[
        Self::Qwen,
        Self::Codex,
        Self::Copilot,
        Self::OpenCode,
        Self::CommandCode,
        Self::Cursor,
        Self::Kilo,
        Self::MiMoCode,
        Self::Droid,
        Self::Oz,
        Self::Antigravity,
        Self::Grok,
        Self::Custom,
    ];

    pub fn parse_str(s: &str) -> Option<Self> {
        match s.to_lowercase().as_str() {
            "qwen" => Some(Self::Qwen),
            "codex" => Some(Self::Codex),
            "copilot" => Some(Self::Copilot),
            "opencode" => Some(Self::OpenCode),
            "commandcode" => Some(Self::CommandCode),
            "cursor" => Some(Self::Cursor),
            "kilo" => Some(Self::Kilo),
            "mimocode" => Some(Self::MiMoCode),
            "droid" => Some(Self::Droid),
            "oz" => Some(Self::Oz),
            "claude" => Some(Self::Claude),
            "agy" => Some(Self::Antigravity),
            "antigravity" => Some(Self::Antigravity),
            "grok" => Some(Self::Grok),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            Self::Qwen => "qwen",
            Self::Codex => "codex",
            Self::Copilot => "copilot",
            Self::OpenCode => "opencode",
            Self::CommandCode => "commandcode",
            Self::Cursor => "cursor",
            Self::Kilo => "kilo",
            Self::MiMoCode => "mimocode",
            Self::Droid => "droid",
            Self::Oz => "oz",
            Self::Claude => "claude",
            Self::Antigravity => "agy",
            Self::Grok => "grok",
            Self::Custom => "custom",
        }
    }

    pub fn sandboxed_fs(&self) -> bool {
        matches!(self, Self::OpenCode)
    }

    /// Whether retries/iterations can resume the prior agent session via a
    /// stored `agent_session_id`. True for agents whose `build_command`
    /// replays a captured session id: the opencode family (opencode, kilo,
    /// mimocode) via `--session`/`--continue`/`--fork`, droid via `-s`,
    /// and Codex via `exec resume`.
    pub fn supports_session_resume(&self) -> bool {
        matches!(
            self,
            Self::OpenCode
                | Self::CommandCode
                | Self::Kilo
                | Self::MiMoCode
                | Self::Droid
                | Self::Codex
                | Self::Qwen
                | Self::Grok
        )
    }

    /// Display metadata: (command, description, cost, best_for, streaming).
    /// Egress is not here — it is a property of the provider (see
    /// [`crate::types::egress_for_cli`]), not of the CLI binary.
    pub fn profile(&self) -> Option<(&'static str, &'static str, &'static str, &'static str, bool)> {
        match self {
            Self::Qwen => Some(profile_qwen()),
            Self::Codex => Some(profile_codex()),
            Self::Copilot => Some(profile_copilot()),
            Self::OpenCode => Some(profile_open_code()),
            Self::CommandCode => Some(profile_command_code()),
            Self::Cursor => Some(profile_cursor()),
            Self::Kilo => Some(profile_kilo()),
            Self::MiMoCode => Some(profile_mi_mo_code()),
            Self::Droid => Some(profile_droid()),
            Self::Oz => Some(profile_oz()),
            Self::Claude => Some(profile_claude()),
            Self::Antigravity => Some(profile_antigravity()),
            Self::Grok => Some(profile_grok()),
            Self::Custom => None,
        }
    }
}

impl fmt::Display for AgentKind {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}
