// Capability matrix and custom-agent scoring helpers for agent selection.
// Exports: AGENT_CAPABILITIES, base/custom scores, team overrides, install checks.
// Deps: classifier categories, custom configs, teams, agent kinds, process lookup.

use std::process::Command;

use crate::agent::classifier::TaskCategory;
use crate::agent::custom::CustomAgentConfig;
use crate::team::TeamConfig;
use crate::types::AgentKind;

pub(super) const AGENT_CAPABILITIES: &[(AgentKind, &[(TaskCategory, i32)])] = &[
    (AgentKind::Gemini, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 6),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Antigravity, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 6),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Grok, &[
        (TaskCategory::Research, 4), (TaskCategory::Documentation, 4),
        (TaskCategory::Debugging, 4), (TaskCategory::SimpleEdit, 4),
        (TaskCategory::ComplexImpl, 4), (TaskCategory::Frontend, 3),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 4),
    ]),
    (AgentKind::Qwen, &[
        (TaskCategory::Research, 8), (TaskCategory::Documentation, 5),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 2),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Frontend, 2),
        (TaskCategory::Testing, 3), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Codex, &[
        (TaskCategory::ComplexImpl, 9), (TaskCategory::Refactoring, 8),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 4), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 4), (TaskCategory::Documentation, 3),
    ]),
    (AgentKind::Copilot, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 6), (TaskCategory::Research, 4),
        (TaskCategory::Frontend, 6), (TaskCategory::Documentation, 5),
    ]),
    (AgentKind::OpenCode, &[
        (TaskCategory::SimpleEdit, 8), (TaskCategory::Documentation, 5),
        (TaskCategory::Testing, 4), (TaskCategory::Debugging, 4),
        (TaskCategory::ComplexImpl, 3), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 4),
    ]),
    (AgentKind::Kilo, &[
        (TaskCategory::SimpleEdit, 7), (TaskCategory::Documentation, 4),
        (TaskCategory::Testing, 3), (TaskCategory::Debugging, 3),
        (TaskCategory::ComplexImpl, 2), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::MiMoCode, &[
        (TaskCategory::SimpleEdit, 7), (TaskCategory::Documentation, 4),
        (TaskCategory::Testing, 3), (TaskCategory::Debugging, 3),
        (TaskCategory::ComplexImpl, 2), (TaskCategory::Research, 1),
        (TaskCategory::Frontend, 2), (TaskCategory::Refactoring, 3),
    ]),
    (AgentKind::Cursor, &[
        (TaskCategory::Frontend, 9), (TaskCategory::ComplexImpl, 7),
        (TaskCategory::Refactoring, 6), (TaskCategory::Testing, 5),
        (TaskCategory::Debugging, 5), (TaskCategory::SimpleEdit, 4),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Codebuff, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Frontend, 7), (TaskCategory::Testing, 6),
        (TaskCategory::Debugging, 6), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::Research, 2), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Droid, &[
        (TaskCategory::ComplexImpl, 9), (TaskCategory::Refactoring, 8),
        (TaskCategory::Testing, 7), (TaskCategory::Debugging, 7),
        (TaskCategory::SimpleEdit, 5), (TaskCategory::Research, 3),
        (TaskCategory::Frontend, 5), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Oz, &[
        (TaskCategory::ComplexImpl, 8), (TaskCategory::Refactoring, 7),
        (TaskCategory::Testing, 6), (TaskCategory::Debugging, 6),
        (TaskCategory::SimpleEdit, 5), (TaskCategory::Research, 3),
        (TaskCategory::Frontend, 6), (TaskCategory::Documentation, 4),
    ]),
    (AgentKind::Claude, &[
        (TaskCategory::Research, 9), (TaskCategory::Documentation, 9),
        (TaskCategory::Debugging, 10), (TaskCategory::SimpleEdit, 5),
        (TaskCategory::ComplexImpl, 10), (TaskCategory::Frontend, 7),
        (TaskCategory::Testing, 10), (TaskCategory::Refactoring, 10),
    ]),
];

pub(super) fn base_score(agent: AgentKind, category: TaskCategory) -> i32 {
    AGENT_CAPABILITIES.iter()
        .find(|(kind, _)| *kind == agent)
        .and_then(|(_, scores)| scores.iter().find(|(item, _)| *item == category))
        .map(|(_, score)| *score)
        .unwrap_or(1)
}

pub(super) fn custom_category_score(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let caps = &config.capabilities;
    match category {
        TaskCategory::Research => caps.research,
        TaskCategory::SimpleEdit => caps.simple_edit,
        TaskCategory::ComplexImpl => caps.complex_impl,
        TaskCategory::Frontend => caps.frontend,
        TaskCategory::Debugging => caps.debugging,
        TaskCategory::Testing => caps.testing,
        TaskCategory::Refactoring => caps.refactoring,
        TaskCategory::Documentation => caps.documentation,
    }
}

pub(super) fn custom_strength_bonus(config: &CustomAgentConfig, category: TaskCategory) -> i32 {
    let key = match category {
        TaskCategory::Research => "research",
        TaskCategory::SimpleEdit => "simple_edit",
        TaskCategory::ComplexImpl => "complex_impl",
        TaskCategory::Frontend => "frontend",
        TaskCategory::Debugging => "debugging",
        TaskCategory::Testing => "testing",
        TaskCategory::Refactoring => "refactoring",
        TaskCategory::Documentation => "documentation",
    };
    if config.strengths.iter().any(|item| item.eq_ignore_ascii_case(key)) { 5 } else { 0 }
}

pub(super) fn custom_command_installed(command: &str) -> bool {
    Command::new("which").arg(command).output()
        .map(|output| output.status.success()).unwrap_or(false)
}

pub(super) fn team_override_score(
    team: &TeamConfig,
    agent_name: &str,
    category: TaskCategory,
) -> Option<i32> {
    let overrides = team.overrides.get(agent_name)?;
    match category {
        TaskCategory::Research => overrides.research,
        TaskCategory::SimpleEdit => overrides.simple_edit,
        TaskCategory::ComplexImpl => overrides.complex_impl,
        TaskCategory::Frontend => overrides.frontend,
        TaskCategory::Debugging => overrides.debugging,
        TaskCategory::Testing => overrides.testing,
        TaskCategory::Refactoring => overrides.refactoring,
        TaskCategory::Documentation => overrides.documentation,
    }
}
