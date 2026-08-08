// Display helpers for `aid agent show` / `list` output.
// Exports: show_builtin_profile and print_custom_summary.
// Deps: custom agent config types, AgentKind, provider egress, std::path.

use crate::agent::custom::CustomAgentConfig;
use crate::agent::egress::resolve_agent_egress;
use crate::types::{egress_for_cli, AgentKind};
use std::path::Path;

pub(super) fn show_builtin_profile(kind: AgentKind) {
    let Some((_, description, cost, best_for, streaming)) = kind.profile() else {
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
    // Egress is a property of the provider, not a per-CLI constant.
    println!("  Egress: {}", egress_for_cli(kind).label());
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
    if let Some(url) = config.base_url.as_deref().filter(|s| !s.is_empty()) {
        println!("  Base URL: {}", url);
    }
    // Hand-set trust_tier is ignored; show provider-derived egress.
    println!("  Egress: {}", resolve_agent_egress(&config.id).label());
    if !config.strengths.is_empty() {
        println!("  Strengths: {}", config.strengths.join(", "));
    }
    println!("  Capabilities:");
    print_capabilities(&config.capabilities);
}

fn print_capabilities(cap: &crate::agent::custom::CapabilityScores) {
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

pub(super) fn show_quota() -> anyhow::Result<()> {
    use crate::rate_limit;
    let limited = rate_limit::rate_limited_agents();
    println!("{:<12} {:<10} DETAIL", "AGENT", "STATUS");
    for kind in AgentKind::ALL_BUILTIN {
        let name = kind.as_str();
        if let Some((_, msg)) = limited.iter().find(|(a, _)| a == name) {
            let info = rate_limit::get_rate_limit_info(kind, None);
            let recovery = info
                .as_ref()
                .and_then(|i| i.recovery_at.as_deref())
                .unwrap_or("~1h");
            println!(
                "{:<12} {:<10} resets {recovery} — {msg}",
                name, "LIMITED"
            );
        } else {
            println!("{:<12} {:<10}", name, "OK");
        }
    }
    for (name, msg) in &limited {
        if AgentKind::parse_str(name).is_some() {
            continue;
        }
        let info = rate_limit::get_rate_limit_info(&AgentKind::Custom, Some(name));
        let recovery = info
            .as_ref()
            .and_then(|i| i.recovery_at.as_deref())
            .unwrap_or("~1h");
        println!("{:<12} {:<10} resets {recovery} — {msg}", name, "LIMITED");
    }
    Ok(())
}

pub(super) fn list_agents() -> anyhow::Result<()> {
    use crate::agent::registry;
    println!("Built-in agents:");
    println!("  {:<10} {:<12} DESCRIPTION", "NAME", "EGRESS");
    for kind in AgentKind::ALL_BUILTIN {
        if let Some((_, description, _, _, _)) = kind.profile() {
            println!(
                "  {:<10} {:<12} {}",
                kind.as_str(),
                egress_for_cli(*kind).label(),
                description
            );
        }
    }
    println!("\nCustom agents:");
    let custom = registry::list_custom_agents();
    if custom.is_empty() {
        println!("  (none installed — use `aid agent add <name>` to create one)");
        return Ok(());
    }
    println!("  {:<10} {:<12} DISPLAY NAME", "NAME", "EGRESS");
    for config in custom {
        println!(
            "  {:<10} {:<12} {}",
            config.id,
            resolve_agent_egress(&config.id).label(),
            config.display_name
        );
    }
    Ok(())
}
