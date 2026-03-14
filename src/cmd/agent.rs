// Handler for `aid agent` subcommands — manage custom agent definitions.
// Exports: AgentAction, run_agent_command.
// Deps: crate::agent::registry, crate::agent::custom, crate::paths, std::fs.
use anyhow::{bail, Context, Result};
use std::fs;
use std::path::{Path, PathBuf};

use crate::agent::custom::{CapabilityScores, CustomAgentConfig, parse_config};
use crate::agent::registry;
use crate::paths;

const AGENT_TEMPLATE: &str = r#"[agent]
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
    description: &'static str,
    cost: &'static str,
    best_for: &'static str,
    streaming: bool,
}

const BUILTIN_AGENT_PROFILES: &[BuiltinAgentProfile] = &[
    BuiltinAgentProfile {
        name: "gemini",
        description: "Research, coding, web search, file editing",
        cost: "$0.10-$10/M blended",
        best_for: "research, explain, implement, create, analyze, build",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "codex",
        description: "Complex implementation, multi-file refactors",
        cost: "$0.10-$8/M blended",
        best_for: "implement, create, build, refactor, test",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "opencode",
        description: "Simple edits, renames, type annotations",
        cost: "free-$2/M blended",
        best_for: "rename, change, update, fix typo, add type",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "cursor",
        description: "General coding, strong model selection, frontend",
        cost: "$20/mo subscription",
        best_for: "implement, create, build, refactor, ui, frontend, css",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "kilo",
        description: "Simple edits (free tier)",
        cost: "free",
        best_for: "rename, change, update, fix typo, add type",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "ob1",
        description: "Multi-model coding, 300+ models",
        cost: "$10/day budget",
        best_for: "research, explain, implement, create, analyze, build",
        streaming: true,
    },
    BuiltinAgentProfile {
        name: "codebuff",
        description: "Complex implementation, frontend",
        cost: "SDK-managed",
        best_for: "complex coding, frontend",
        streaming: true,
    },
];

pub enum AgentAction {
    List,
    Show { name: String },
    Add { name: String },
    Remove { name: String },
}

pub fn run_agent_command(action: AgentAction) -> Result<()> {
    match action {
        AgentAction::List => list_agents(),
        AgentAction::Show { name } => show_agent(&name),
        AgentAction::Add { name } => add_agent(&name),
        AgentAction::Remove { name } => remove_agent(&name),
    }
}

fn list_agents() -> Result<()> {
    println!("Built-in agents:");
    for profile in BUILTIN_AGENT_PROFILES {
        println!("  {:<10} {}", profile.name, profile.description);
    }
    println!("\nCustom agents:");
    let custom = registry::list_custom_agents();
    if custom.is_empty() {
        println!("  (none installed — use `aid agent add <name>` to create one)");
        return Ok(());
    }
    for config in custom {
        let path = custom_agent_path(&config.id);
        println!(
            "  {:<10} {:<40}{}",
            config.id,
            config.display_name,
            path.display()
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
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!("Custom agent '{name}' does not exist");
    }
    fs::remove_file(&target)?;
    println!("Removed {}", target.display());
    Ok(())
}

fn show_builtin_profile(profile: &BuiltinAgentProfile) {
    println!("Built-in agent: {}", profile.name);
    println!("  Description: {}", profile.description);
    println!("  Cost: {}", profile.cost);
    println!("  Best for: {}", profile.best_for);
    println!("  Mode: {}", if profile.streaming { "streaming" } else { "buffered" });
}

fn show_custom_agent(name: &str) -> Result<()> {
    let target = custom_agent_path(name);
    if !target.is_file() {
        bail!("Custom agent '{name}' not found (expected at {})", target.display());
    }
    let contents = fs::read_to_string(&target)
        .with_context(|| format!("Failed to read {}", target.display()))?;
    let config = parse_config(&contents)
        .with_context(|| format!("Failed to parse {}", target.display()))?;
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
