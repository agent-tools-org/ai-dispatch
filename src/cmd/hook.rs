// Hook subcommands for lightweight Claude Code integration.
// Exports: session_start; depends on crate::project and crate::team.

use anyhow::Result;
use chrono::NaiveDateTime;

use crate::project::{self, ProjectConfig};
use crate::team::TeamConfig;
use crate::types::AgentKind;

const BASE_TEXT: &str = "[aid] ai-dispatch is installed for multi-agent orchestration. Key commands:
- Dispatch: aid run <agent> \"<prompt>\" [--worktree <branch>]
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
    println!("{rendered}");
    Ok(())
}

enum QuotaState {
    Ok,
    Limited { resets: Option<NaiveDateTime> },
}

fn agents_status_line() -> Option<String> {
    let installed = crate::agent::detect_agents();
    let entries = AgentKind::ALL_BUILTIN
        .iter()
        .copied()
        .filter(|kind| installed.contains(kind))
        .filter(|kind| !crate::agent_config::is_agent_disabled(kind.as_str()))
        .map(|kind| {
            let state = if crate::rate_limit::is_rate_limited(&kind) {
                QuotaState::Limited { resets: crate::rate_limit::recovery_datetime(&kind) }
            } else {
                QuotaState::Ok
            };
            (kind.as_str().to_string(), state)
        })
        .collect::<Vec<_>>();
    render_agents_status_line(&entries)
}

fn render_agents_status_line(entries: &[(String, QuotaState)]) -> Option<String> {
    let any_limited = entries
        .iter()
        .any(|(_, state)| matches!(state, QuotaState::Limited { .. }));
    if !any_limited {
        return None;
    }
    let parts = entries
        .iter()
        .map(|(name, state)| match state {
            QuotaState::Ok => format!("{name} ok"),
            QuotaState::Limited { resets: Some(time) } => {
                format!("{name} LIMITED (resets {})", time.format("%H:%M"))
            }
            QuotaState::Limited { resets: None } => {
                format!("{name} LIMITED")
            }
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
mod tests {
    use super::{QuotaState, render_agents_status_line, render_session_start};
    use crate::project::{ProjectAgents, ProjectBudget, ProjectConfig};
    use crate::team::TeamConfig;
    use std::collections::HashMap;

    fn entry(name: &str, state: QuotaState) -> (String, QuotaState) {
        (name.to_string(), state)
    }

    #[test]
    fn agent_line_silent_when_all_ok() {
        let entries = vec![
            entry("codex", QuotaState::Ok),
            entry("agy", QuotaState::Ok),
        ];
        assert_eq!(render_agents_status_line(&entries), None);
    }

    #[test]
    fn agent_line_formats_limited_fleet() {
        let resets = chrono::NaiveDateTime::parse_from_str("2026-08-05 18:27", "%Y-%m-%d %H:%M")
            .expect("valid datetime");
        let entries = vec![
            entry("codex", QuotaState::Limited { resets: Some(resets) }),
            entry("agy", QuotaState::Ok),
            entry("opencode", QuotaState::Ok),
        ];
        assert_eq!(
            render_agents_status_line(&entries).as_deref(),
            Some("agents: codex LIMITED (resets 18:27) - agy ok - opencode ok")
        );
    }

    #[test]
    fn agent_line_omits_reset_time_when_unknown() {
        let entries = vec![
            entry("codex", QuotaState::Limited { resets: None }),
            entry("agy", QuotaState::Ok),
        ];
        assert_eq!(
            render_agents_status_line(&entries).as_deref(),
            Some("agents: codex LIMITED - agy ok")
        );
    }

    #[test]
    fn agents_status_line_reads_markers_for_installed_fleet() {
        let temp_dir = std::env::temp_dir().join("aid-hook-quota-line-test");
        let _home = crate::paths::AidHomeGuard::set(&temp_dir);
        std::fs::create_dir_all(crate::paths::aid_dir()).expect("create aid dir");
        let _fleet = crate::agent::DetectAgentsGuard::set(vec![
            crate::types::AgentKind::Codex,
            crate::types::AgentKind::Antigravity,
            crate::types::AgentKind::OpenCode,
        ]);
        let recovery = (chrono::Local::now() + chrono::Duration::hours(2)).naive_local();
        let marker = format!(
            "recovery_at: {}\nmessage: quota exhausted\n",
            recovery.format("%b %d, %Y %I:%M %p")
        );
        std::fs::write(crate::paths::aid_dir().join("rate-limit-codex"), marker)
            .expect("write marker");

        let line = super::agents_status_line().expect("line when an agent is limited");
        let expected_reset = recovery.format("%H:%M");
        assert_eq!(
            line,
            format!("agents: codex LIMITED (resets {expected_reset}) - opencode ok - agy ok")
        );

        std::fs::remove_file(crate::paths::aid_dir().join("rate-limit-codex")).ok();
        assert_eq!(super::agents_status_line(), None);
    }

    #[test]
    fn renders_base_text_without_project() {
        let rendered = render_session_start(None, None);
        assert!(rendered.contains("[aid] ai-dispatch is installed"));
        assert!(!rendered.contains("Project:"));
        assert!(!rendered.contains("Rules:"));
        assert!(rendered.contains("aid project init"));
    }

    #[test]
    fn renders_project_and_combined_rule_count() {
        let project = ProjectConfig {
            id: "ai-dispatch".to_string(),
            profile: Some("standard".to_string()),
            max_task_cost: None,
            team: Some("dev".to_string()),
            verify: None,
            setup: None,
            container: None,
            gitbutler: None,
            language: None,
            rules: vec!["project rule".to_string()],
            budget: ProjectBudget::default(),
            agents: ProjectAgents::default(),
            audit: Default::default(),
            ..Default::default()
        };
        let team = TeamConfig {
            id: "dev".to_string(),
            display_name: "Dev".to_string(),
            description: String::new(),
            preferred_agents: vec![],
            default_agent: None,
            overrides: HashMap::new(),
            rules: vec!["team rule 1".to_string(), "team rule 2".to_string()],
            toolbox: Default::default(),
        };

        let rendered = render_session_start(Some(&project), Some(&team));

        assert!(rendered.contains("Project: ai-dispatch (profile: standard, team: dev)"));
        assert!(rendered.contains("Rules: 3 rule(s)"));
    }
}
