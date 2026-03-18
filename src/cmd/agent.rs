// Handler for `aid agent` subcommands — manage custom agent definitions.
// Exports: AgentAction, run_agent_command.
// Deps: crate::agent::registry, crate::agent::custom, crate::paths, std::fs.
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::classifier::TaskCategory;
use crate::agent::custom::{parse_config, CapabilityScores, CustomAgentConfig};
use crate::agent::registry;
use crate::agent::selection::AGENT_CAPABILITIES;
use crate::paths;
use crate::sanitize;
use crate::types::AgentKind;

const AGENT_TEMPLATE: &str = r#"# Custom agent definition for aid.
#
# Requirements: the command must be a non-interactive CLI that:
#   1. Accepts a prompt (via arg, flag, or stdin)
#   2. Performs the task autonomously
#   3. Exits when done
#
# Compatible CLIs: gemini, codex, opencode, cursor, kilo, codebuff, aider, etc.
# NOT compatible: interactive/session-based tools (e.g. Claude Code) — those
# are orchestrators that call aid, not agents that aid dispatches.

[agent]
id = "{name}"
display_name = "{display_name}"
command = "{name}"

# How to pass the prompt
prompt_mode = "arg"     # "arg", "stdin", or "flag"
# prompt_flag = "--message"  # uncomment if prompt_mode = "flag"

# CLI flag mappings (leave empty if not supported)
dir_flag = ""
model_flag = "--model"
output_flag = ""

# Fixed args always passed to the CLI
fixed_args = []

# Output parsing
streaming = false
output_format = "text"  # "text" or "jsonl"
# Strength categories for simple boosts (match TaskCategory strings, e.g. "research")
strengths = []

# Trust tier: "local" (runs locally) or "api" (sends prompts to third-party)
trust_tier = "api"

# Capability scores for auto-selection (0-10)
[agent.capabilities]
research = 3
simple_edit = 5
complex_impl = 5
frontend = 3
debugging = 5
testing = 4
refactoring = 5
documentation = 3
"#;

struct BuiltinAgentProfile {
    name: &'static str,
    command: &'static str,
    description: &'static str,
    cost: &'static str,
    best_for: &'static str,
    streaming: bool,
    trust_tier: &'static str,
}

const BUILTIN_AGENT_PROFILES: &[BuiltinAgentProfile] = &[
    BuiltinAgentProfile {
        name: "gemini",
        command: "gemini",
        description: "Research, coding, web search, file editing",
        cost: "$0.10-$10/M blended",
        best_for: "research, explain, implement, create, analyze, build",
        streaming: true,
        trust_tier: "api",
    },
    BuiltinAgentProfile {
        name: "codex",
        command: "codex",
        description: "Complex implementation, multi-file refactors",
        cost: "$0.10-$8/M blended",
        best_for: "implement, create, build, refactor, test",
        streaming: true,
        trust_tier: "local",
    },
    BuiltinAgentProfile {
        name: "opencode",
        command: "opencode",
        description: "Simple edits, renames, type annotations",
        cost: "free-$2/M blended",
        best_for: "rename, change, update, fix typo, add type",
        streaming: true,
        trust_tier: "api",
    },
    BuiltinAgentProfile {
        name: "cursor",
        command: "cursor",
        description: "General coding, strong model selection, frontend",
        cost: "$20/mo subscription",
        best_for: "implement, create, build, refactor, ui, frontend, css",
        streaming: true,
        trust_tier: "api",
    },
    BuiltinAgentProfile {
        name: "kilo",
        command: "kilo",
        description: "Simple edits (free tier)",
        cost: "free",
        best_for: "rename, change, update, fix typo, add type",
        streaming: true,
        trust_tier: "api",
    },
    BuiltinAgentProfile {
        name: "codebuff",
        command: "aid-codebuff",
        description: "Complex implementation, frontend",
        cost: "SDK-managed",
        best_for: "complex coding, frontend",
        streaming: true,
        trust_tier: "local",
    },
    BuiltinAgentProfile {
        name: "droid",
        command: "droid",
        description: "Complex implementation, multi-agent orchestration",
        cost: "BYOK (API key)",
        best_for: "implement, create, build, refactor, test, orchestrate",
        streaming: true,
        trust_tier: "api",
    },
];

pub enum AgentAction {
    List,
    Show {
        name: String,
    },
    Add {
        name: String,
    },
    Remove {
        name: String,
    },
    Fork {
        name: String,
        new_name: Option<String>,
    },
    Quota,
}

pub fn run_agent_command(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::List => list_agents(),
        AgentAction::Show { name } => show_agent(&name),
        AgentAction::Add { name } => add_agent(&name),
        AgentAction::Remove { name } => remove_agent(&name),
        AgentAction::Fork { name, new_name } => fork_agent(&name, new_name.as_deref()),
        AgentAction::Quota => show_quota(),
    }
}

fn show_quota() -> Result<()> {
    use crate::rate_limit;
    let limited = rate_limit::rate_limited_agents();
    println!("{:<12} {:<10} DETAIL", "AGENT", "STATUS");
    for profile in BUILTIN_AGENT_PROFILES {
        let kind = AgentKind::parse_str(profile.name).unwrap_or(AgentKind::Codex);
        if let Some((_, msg)) = limited.iter().find(|(a, _)| *a == kind) {
            let info = rate_limit::get_rate_limit_info(&kind);
            let recovery = info
                .as_ref()
                .and_then(|i| i.recovery_at.as_deref())
                .unwrap_or("~1h");
            println!(
                "{:<12} {:<10} resets {recovery} — {msg}",
                profile.name, "LIMITED"
            );
        } else {
            println!("{:<12} {:<10}", profile.name, "OK");
        }
    }
    Ok(())
}

fn list_agents() -> Result<()> {
    println!("Built-in agents:");
    println!("  {:<10} {:<6} DESCRIPTION", "NAME", "TRUST");
    for profile in BUILTIN_AGENT_PROFILES {
        println!(
            "  {:<10} {:<6} {}",
            profile.name, profile.trust_tier, profile.description
        );
    }
    println!("\nCustom agents:");
    let custom = registry::list_custom_agents();
    if custom.is_empty() {
        println!("  (none installed — use `aid agent add <name>` to create one)");
        return Ok(());
    }
    println!("  {:<10} {:<6} DISPLAY NAME", "NAME", "TRUST");
    for config in custom {
        println!(
            "  {:<10} {:<6} {}",
            config.id, config.trust_tier, config.display_name
        );
    }
    Ok(())
}

fn show_agent(name: &str) -> Result<()> {
    if let Some(profile) = builtin_profile(name) {
        show_builtin_profile(profile);
        return Ok(());
    }
    show_custom_agent(name)
}

fn add_agent(name: &str) -> Result<()> {
    if is_builtin(name) {
        bail!("Cannot add custom agent '{name}' because it conflicts with a built-in agent");
    }
    let dir = agent_dir();
    fs::create_dir_all(&dir)?;
    let target = custom_agent_path(name);
    if target.exists() {
        bail!("Agent '{name}' already exists at {}", target.display());
    }
    let display_name = title_case(name);
    let contents = AGENT_TEMPLATE
        .replace("{name}", name)
        .replace("{display_name}", &display_name);
    fs::write(&target, contents)?;
    println!("Created {}", target.display());
    println!("Edit the file to configure the agent.");
    Ok(())
}

fn remove_agent(name: &str) -> Result<()> {
    if is_builtin(name) {
        bail!("Cannot remove built-in agent '{name}'");
    }
    sanitize::validate_name(name, "agent")?;
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!("Custom agent '{name}' does not exist");
    }
    fs::remove_file(&target)?;
    println!("Removed {}", target.display());
    Ok(())
}

fn fork_agent(name: &str, new_name: Option<&str>) -> Result<()> {
    let target_name = new_name
        .map(|value| value.to_string())
        .unwrap_or_else(|| format!("{name}-custom"));
    if is_builtin(&target_name) {
        bail!("Cannot fork into '{target_name}' because it conflicts with a built-in agent");
    }

    let dir = agent_dir();
    fs::create_dir_all(&dir)?;
    let target = custom_agent_path(&target_name);
    if target.exists() {
        bail!(
            "Agent '{target_name}' already exists at {}",
            target.display()
        );
    }

    if let Some(profile) = builtin_profile(name) {
        let contents = build_builtin_agent_toml(&target_name, profile);
        fs::write(&target, contents)
            .with_context(|| format!("Failed to write {}", target.display()))?;
        println!("Created {}", target.display());
        println!("Edit the file to configure the agent.");
        return Ok(());
    }

    let source = custom_agent_path(name);
    if !source.is_file() {
        bail!(
            "Custom agent '{name}' not found (expected at {})",
            source.display()
        );
    }
    fs::copy(&source, &target).with_context(|| {
        format!(
            "Failed to copy {} to {}",
            source.display(),
            target.display()
        )
    })?;
    println!("Created {}", target.display());
    println!("Edit the file to configure the agent.");
    Ok(())
}

fn show_builtin_profile(profile: &BuiltinAgentProfile) {
    println!("Built-in agent: {}", profile.name);
    println!("  Description: {}", profile.description);
    println!("  Cost: {}", profile.cost);
    println!("  Best for: {}", profile.best_for);
    println!(
        "  Mode: {}",
        if profile.streaming {
            "streaming"
        } else {
            "buffered"
        }
    );
    println!("  Trust tier: {}", profile.trust_tier);
}

fn show_custom_agent(name: &str) -> Result<()> {
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!(
            "Custom agent '{name}' not found (expected at {})",
            target.display()
        );
    }
    let contents = fs::read_to_string(&target)
        .with_context(|| format!("Failed to read {}", target.display()))?;
    let config =
        parse_config(&contents).with_context(|| format!("Failed to parse {}", target.display()))?;
    print_custom_summary(&config, &target);
    println!("\nTOML preview:\n{}", contents);
    Ok(())
}

fn print_custom_summary(config: &CustomAgentConfig, path: &Path) {
    println!("Custom agent: {}", config.id);
    println!("  File: {}", path.display());
    println!("  Display name: {}", config.display_name);
    println!("  Command: {}", config.command);
    println!("  Prompt mode: {}", config.prompt_mode);
    println!("  Prompt flag: {}", config.prompt_flag);
    println!("  Dir flag: {}", config.dir_flag);
    println!("  Model flag: {}", config.model_flag);
    println!("  Output flag: {}", config.output_flag);
    if config.fixed_args.is_empty() {
        println!("  Fixed args: (none)");
    } else {
        println!("  Fixed args: {}", config.fixed_args.join(" "));
    }
    println!("  Streaming: {}", config.streaming);
    println!("  Output format: {}", config.output_format);
    println!("  Trust tier: {}", config.trust_tier);
    if !config.strengths.is_empty() {
        println!("  Strengths: {}", config.strengths.join(", "));
    }
    println!("  Capabilities:");
    print_capabilities(&config.capabilities);
}

fn print_capabilities(cap: &CapabilityScores) {
    for (label, value) in &[
        ("research", cap.research),
        ("simple_edit", cap.simple_edit),
        ("complex_impl", cap.complex_impl),
        ("frontend", cap.frontend),
        ("debugging", cap.debugging),
        ("testing", cap.testing),
        ("refactoring", cap.refactoring),
        ("documentation", cap.documentation),
    ] {
        println!("    {:<12} {}", label, value);
    }
}

fn builtin_profile(name: &str) -> Option<&'static BuiltinAgentProfile> {
    BUILTIN_AGENT_PROFILES
        .iter()
        .find(|profile| profile.name.eq_ignore_ascii_case(name))
}

fn is_builtin(name: &str) -> bool {
    builtin_profile(name).is_some()
}

fn agent_dir() -> PathBuf {
    paths::aid_dir().join("agents")
}

fn custom_agent_path(name: &str) -> PathBuf {
    agent_dir().join(format!("{name}.toml"))
}

fn build_builtin_agent_toml(target_name: &str, profile: &BuiltinAgentProfile) -> String {
    let display_name = title_case(target_name);
    let kind = AgentKind::parse_str(profile.name).unwrap_or(AgentKind::Codex);
    let caps = capability_scores_for(kind);
    let mut toml = String::new();
    toml.push_str(&format!(
        "# Forked from the built-in `{}` agent. Edit the entries below to customize this clone.\n",
        profile.name
    ));
    toml.push_str("[agent]\n");
    toml.push_str(&format!("id = \"{target_name}\"\n"));
    toml.push_str(&format!("display_name = \"{display_name}\"\n"));
    toml.push_str(&format!(
        "command = \"{}\"  # CLI binary invoked by this agent\n",
        profile.command
    ));
    toml.push_str("\n# How prompts reach the CLI\n");
    toml.push_str("prompt_mode = \"arg\"  # options: arg | flag | stdin\n");
    toml.push_str("# prompt_flag = \"--message\"  # enable when prompt_mode = \"flag\"\n\n");
    toml.push_str("# Optional CLI flags for directory, model, and output\n");
    toml.push_str("dir_flag = \"\"  # e.g. --dir or --workspace\n");
    toml.push_str("model_flag = \"\"  # e.g. --model\n");
    toml.push_str("output_flag = \"\"  # e.g. --output\n\n");
    toml.push_str("# Arguments that always run with this agent\n");
    toml.push_str("fixed_args = []\n\n");
    toml.push_str("# Streaming controls whether aid expects live JSONL events\n");
    toml.push_str(&format!("streaming = {}\n", profile.streaming));
    toml.push_str("output_format = \"text\"  # text | jsonl\n\n");
    toml.push_str(
        "# Trust tier: \"local\" (runs locally) or \"api\" (sends prompts to third-party)\n",
    );
    toml.push_str(&format!("trust_tier = \"{}\"\n\n", profile.trust_tier));
    toml.push_str("# Strength categories for auto-selection boosts\n");
    toml.push_str("strengths = []\n\n");
    toml.push_str("# Capability scores (0-10) guide auto-selection\n");
    toml.push_str("[agent.capabilities]\n");
    toml.push_str(&format!("research = {}\n", caps.research));
    toml.push_str(&format!("simple_edit = {}\n", caps.simple_edit));
    toml.push_str(&format!("complex_impl = {}\n", caps.complex_impl));
    toml.push_str(&format!("frontend = {}\n", caps.frontend));
    toml.push_str(&format!("debugging = {}\n", caps.debugging));
    toml.push_str(&format!("testing = {}\n", caps.testing));
    toml.push_str(&format!("refactoring = {}\n", caps.refactoring));
    toml.push_str(&format!("documentation = {}\n", caps.documentation));
    toml.push('\n');
    toml
}

fn capability_scores_for(kind: AgentKind) -> CapabilityScores {
    let mut scores = CapabilityScores::default();
    if let Some((_, entries)) = AGENT_CAPABILITIES.iter().find(|(k, _)| *k == kind) {
        for &(category, value) in *entries {
            match category {
                TaskCategory::Research => scores.research = value,
                TaskCategory::SimpleEdit => scores.simple_edit = value,
                TaskCategory::ComplexImpl => scores.complex_impl = value,
                TaskCategory::Frontend => scores.frontend = value,
                TaskCategory::Debugging => scores.debugging = value,
                TaskCategory::Testing => scores.testing = value,
                TaskCategory::Refactoring => scores.refactoring = value,
                TaskCategory::Documentation => scores.documentation = value,
            }
        }
    }
    scores
}

fn title_case(name: &str) -> String {
    let pieces: Vec<String> = name
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            let first = chars.next();
            match first {
                Some(f) => f.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if pieces.is_empty() {
        return name.to_string();
    }
    pieces.join(" ")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::paths;

    #[test]
    fn show_quota_runs_without_panic() {
        let temp_dir = std::env::temp_dir().join("aid-quota-test-no-markers");
        let _guard = paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(paths::aid_dir()).ok(); // No rate-limit markers — should not panic
        let result = show_quota();
        assert!(result.is_ok());
    }
}
