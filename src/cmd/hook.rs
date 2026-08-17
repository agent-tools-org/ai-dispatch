// Hook subcommands for lightweight Claude Code integration.
// Exports: session_start; depends on crate::project and crate::team.

use anyhow::Result;
use chrono::NaiveDateTime;

use crate::project::{self, ProjectConfig};
use crate::team::TeamConfig;
use crate::types::AgentKind;

/// Injected into every session, so it is the one place a dispatcher reliably
/// reads. It carries the practices that were paid for, not a command list the
/// caller could get from `--help`.
const BASE_TEXT: &str = "[aid] ai-dispatch is installed for multi-agent orchestration.

You are the dispatcher, and aid does not guess what you already know:
- Declare the profile: --difficulty --budget --urgency --rigor. Undeclared is stored as
  null, not inferred.
- Declare --skill; aid picks none for you. Declare --kind to narrow the injected toolbox;
  omit it and every tool is described, because omission is not a decision.
- A route is <cli>/<provider>/<model>. An exhausted route says nothing about another
  provider reaching a model of the same class. `aid agent list --json` carries both, plus
  the metering shape that decides whether an outage recovers with time at all.
- Do not dispatch to a weaker model on the provider pool you are already running on. A
  different provider is delegation; the same pool for a worse model is waste.
- Judge a delivery by running it, not by reading its diff. --rigor sets the proof owed:
  draft compiles, standard runs the changed path, critical adds an independent audit.
- Keep briefs short: the goal and the red lines, not the implementation path.
- Do not edit a directory while an agent works in it. AID snapshots dirty paths once, at
  dispatch, so edits made before it are protected from the rescue commit and edits made
  during the run are not. -w <branch> puts the agent somewhere else entirely.

Commands:
- Dispatch: aid run <agent> \"<prompt>\" [--worktree <branch>]
- Compare:  aid advise \"<task>\" --difficulty <d> --budget <b> --urgency <u> --rigor <r>
- Monitor:  aid watch --tui (dashboard) | aid watch --wait <id> (blocking)
- Review:   aid show <id> --diff | aid board
- Batch:    aid batch <file> --parallel";

pub fn session_start() -> Result<()> {
    let project = project::detect_project();
    let team = project
        .as_ref()
        .and_then(|config| config.team.as_deref())
        .and_then(crate::team::resolve_team);
    let mut rendered = render_session_start(project.as_ref(), team.as_ref());
    if let Some(line) = agents_status_line() {
        rendered.push('\n');
        rendered.push_str(&line);
    }
    if let Some(line) = crate::cmd::clean::session_start_hint()? {
        rendered.push('\n');
        rendered.push_str(&line);
    }
    println!("{rendered}");
    Ok(())
}

enum QuotaState {
    Ok { used_percent: Option<f64>, stale: bool },
    Partial { used_percent: Option<f64>, stale: bool },
    Limited {
        resets: Option<NaiveDateTime>,
        used_percent: Option<f64>,
        stale: bool,
    },
}

fn probe_bits(
    probe: Option<&crate::route_availability::ProbeEvidence>,
) -> (Option<f64>, bool) {
    match probe {
        Some(probe) if probe.ok && !probe.windows.is_empty() => {
            let used = probe
                .windows
                .iter()
                .map(|window| window.used_percent)
                .fold(0.0, f64::max);
            (Some(used), probe.stale)
        }
        Some(probe) => (None, probe.stale),
        None => (None, false),
    }
}

fn quota_state_for(kind: AgentKind, custom: Option<&str>) -> QuotaState {
    let avail = crate::route_availability::availability(&kind, custom);
    let (used_percent, stale) = probe_bits(avail.probe.as_ref());
    if avail.status == crate::route_availability::RouteStatus::Held {
        let resets = match avail.ends {
            crate::route_availability::HoldEnd::At(at) => Some(at),
            _ => crate::rate_limit::recovery_datetime(&kind, custom),
        };
        return QuotaState::Limited {
            resets,
            used_percent,
            stale,
        };
    }
    if !crate::rate_limit::active_group_holds(&kind, custom).is_empty() {
        return QuotaState::Partial {
            used_percent,
            stale,
        };
    }
    QuotaState::Ok {
        used_percent,
        stale,
    }
}

fn agents_status_line() -> Option<String> {
    let installed = crate::agent::detect_agents();
    let mut entries = AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .filter(|kind| installed.contains(kind))
        .filter(|kind| !crate::agent_config::is_agent_disabled(kind.as_str()))
        .map(|kind| (kind.as_str().to_string(), quota_state_for(kind, None)))
        .collect::<Vec<_>>();
    for config in crate::agent::registry::list_custom_agents() {
        if crate::agent_config::is_agent_disabled(&config.id) {
            continue;
        }
        let state = quota_state_for(AgentKind::Custom, Some(config.id.as_str()));
        entries.push((config.id, state));
    }
    render_agents_status_line(&entries)
}

fn suffix(used_percent: Option<f64>, stale: bool) -> String {
    let mut out = String::new();
    if let Some(used) = used_percent {
        out.push_str(&format!(" ({used:.0}%)"));
    }
    if stale {
        out.push_str(" STALE");
    }
    out
}

fn render_agents_status_line(entries: &[(String, QuotaState)]) -> Option<String> {
    let any_hold = entries.iter().any(|(_, state)| {
        matches!(state, QuotaState::Limited { .. } | QuotaState::Partial { .. })
    });
    if !any_hold {
        return None;
    }
    let parts = entries
        .iter()
        .map(|(name, state)| match state {
            QuotaState::Ok {
                used_percent,
                stale,
            } => format!("{name} ok{}", suffix(*used_percent, *stale)),
            QuotaState::Partial {
                used_percent,
                stale,
            } => format!("{name} PARTIAL{}", suffix(*used_percent, *stale)),
            QuotaState::Limited {
                resets: Some(time),
                used_percent,
                stale,
            } => format!(
                "{name} LIMITED (resets {}){}",
                time.format("%H:%M"),
                suffix(*used_percent, *stale)
            ),
            QuotaState::Limited {
                resets: None,
                used_percent,
                stale,
            } => format!("{name} LIMITED{}", suffix(*used_percent, *stale)),
        })
        .collect::<Vec<_>>();
    Some(format!("agents: {}", parts.join(" - ")))
}

fn render_session_start(project: Option<&ProjectConfig>, team: Option<&TeamConfig>) -> String {
    let mut lines = vec![BASE_TEXT.to_string()];
    if let Some(config) = project {
        let profile = config.profile.as_deref().unwrap_or("none");
        let team_id = team
            .map(|resolved| resolved.id.as_str())
            .or(config.team.as_deref())
            .unwrap_or("none");
        let rules = config.rules.len() + team.map_or(0, |resolved| resolved.rules.len());
        lines.push(format!(
            "Project: {} (profile: {}, team: {})",
            config.id, profile, team_id
        ));
        lines.push(format!("Rules: {rules} rule(s)"));
    } else {
        lines.push("Tip: run `aid project init` to configure this project for aid orchestration".to_string());
    }
    lines.join("\n")
}

#[cfg(test)]
#[path = "hook_tests.rs"]
mod tests;
