// Display helpers for `aid agent show` / `list` output.
// Exports: show_builtin_profile and print_custom_summary.
// Deps: custom agent config types, AgentKind, provider egress, std::path.

use crate::agent::custom::CustomAgentConfig;
use crate::agent::egress::resolve_agent_egress;
use crate::rate_limit::{self, RateLimitInfo};
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

/// Observed quota state for display. A group hold is Partial, never Limited —
/// the agent remains dispatchable on its clear tiers.
#[derive(Debug, PartialEq)]
enum QuotaRow {
    Ok,
    Limited { detail: String },
    Partial { detail: String },
}

fn quota_row(kind: AgentKind, custom_name: Option<&str>) -> QuotaRow {
    if rate_limit::is_rate_limited(&kind, custom_name) {
        let info = rate_limit::get_rate_limit_info(&kind, custom_name);
        return QuotaRow::Limited {
            detail: agent_hold_detail(&kind, custom_name, info.as_ref()),
        };
    }
    let groups = rate_limit::active_group_holds(&kind, custom_name);
    if groups.is_empty() {
        return QuotaRow::Ok;
    }
    QuotaRow::Partial {
        detail: group_holds_detail(&kind, custom_name, &groups),
    }
}

fn agent_hold_detail(
    kind: &AgentKind,
    custom_name: Option<&str>,
    info: Option<&RateLimitInfo>,
) -> String {
    let Some(info) = info else {
        return String::new();
    };
    let end = rate_limit::format_hold_end(kind, custom_name, info);
    match info.message.as_deref() {
        Some(msg) if !msg.is_empty() => format!("{end} — {msg}"),
        _ => end,
    }
}

fn group_holds_detail(
    kind: &AgentKind,
    custom_name: Option<&str>,
    groups: &[(String, RateLimitInfo)],
) -> String {
    groups
        .iter()
        .map(|(group, info)| {
            let end = rate_limit::format_hold_end(kind, custom_name, info);
            match info.message.as_deref() {
                Some(msg) if !msg.is_empty() => format!("{group} {end} — {msg}"),
                _ => format!("{group} {end}"),
            }
        })
        .collect::<Vec<_>>()
        .join("; ")
}

pub(super) fn show_quota() -> anyhow::Result<()> {
    use crate::agent::registry;
    println!("{:<12} {:<10} DETAIL", "AGENT", "STATUS");
    for kind in AgentKind::ALL_BUILTIN {
        match quota_row(*kind, None) {
            QuotaRow::Ok => println!("{:<12} {:<10}", kind.as_str(), "OK"),
            QuotaRow::Limited { detail } => {
                println!("{:<12} {:<10} {detail}", kind.as_str(), "LIMITED");
            }
            QuotaRow::Partial { detail } => {
                println!("{:<12} {:<10} {detail}", kind.as_str(), "PARTIAL");
            }
        }
    }
    for config in registry::list_custom_agents() {
        match quota_row(AgentKind::Custom, Some(config.id.as_str())) {
            QuotaRow::Ok => println!("{:<12} {:<10}", config.id, "OK"),
            QuotaRow::Limited { detail } => {
                println!("{:<12} {:<10} {detail}", config.id, "LIMITED");
            }
            QuotaRow::Partial { detail } => {
                println!("{:<12} {:<10} {detail}", config.id, "PARTIAL");
            }
        }
    }
    Ok(())
}

pub(super) fn list_agents() -> anyhow::Result<()> {
    use crate::agent::registry;
    let rows: Vec<_> = AgentKind::ALL_BUILTIN
        .iter()
        .filter_map(|kind| kind.profile().map(|_| (*kind, quota_row(*kind, None))))
        .collect();
    let any_hold = rows.iter().any(|(_, row)| !matches!(row, QuotaRow::Ok));

    println!("Built-in agents:");
    if any_hold {
        println!("  {:<10} {:<12} {:<10} DESCRIPTION", "NAME", "EGRESS", "STATUS");
    } else {
        println!("  {:<10} {:<12} DESCRIPTION", "NAME", "EGRESS");
    }
    for (kind, row) in &rows {
        let Some((_, description, _, _, _)) = kind.profile() else {
            continue;
        };
        if any_hold {
            let status = match row {
                QuotaRow::Ok => "",
                QuotaRow::Limited { .. } => "LIMITED",
                QuotaRow::Partial { .. } => "PARTIAL",
            };
            println!(
                "  {:<10} {:<12} {:<10} {}",
                kind.as_str(),
                egress_for_cli(*kind).label(),
                status,
                description
            );
        } else {
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
    let custom_rows: Vec<_> = custom
        .iter()
        .map(|c| (c, quota_row(AgentKind::Custom, Some(c.id.as_str()))))
        .collect();
    let any_custom_hold = custom_rows.iter().any(|(_, row)| !matches!(row, QuotaRow::Ok));
    if any_custom_hold {
        println!("  {:<10} {:<12} {:<10} DISPLAY NAME", "NAME", "EGRESS", "STATUS");
    } else {
        println!("  {:<10} {:<12} DISPLAY NAME", "NAME", "EGRESS");
    }
    for (config, row) in &custom_rows {
        if any_custom_hold {
            let status = match row {
                QuotaRow::Ok => "",
                QuotaRow::Limited { .. } => "LIMITED",
                QuotaRow::Partial { .. } => "PARTIAL",
            };
            println!(
                "  {:<10} {:<12} {:<10} {}",
                config.id,
                resolve_agent_egress(&config.id).label(),
                status,
                config.display_name
            );
        } else {
            println!(
                "  {:<10} {:<12} {}",
                config.id,
                resolve_agent_egress(&config.id).label(),
                config.display_name
            );
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "agent_display_tests.rs"]
mod tests;
