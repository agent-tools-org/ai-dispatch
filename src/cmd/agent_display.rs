// Display helpers for `aid agent show` output.
// Exports: show_builtin_profile and print_custom_summary.
// Deps: custom agent config types, AgentKind, std::path.

use crate::agent::custom::{CapabilityScores, CustomAgentConfig};
use crate::types::AgentKind;
use std::path::Path;

pub(super) fn show_builtin_profile(kind: AgentKind) {
    let Some((_, description, cost, best_for, streaming, trust_tier)) = kind.profile() else {
        return;
    };
    println!("Built-in agent: {}", kind.as_str());
    println!("  Description: {}", description);
    println!("  Cost: {}", cost);
    println!("  Best for: {}", best_for);
    println!(
        "  Mode: {}",
        if streaming {
            "streaming"
        } else {
            "buffered"
        }
    );
    println!("  Trust tier: {}", trust_tier);
}

pub(super) fn print_custom_summary(config: &CustomAgentConfig, path: &Path) {
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
