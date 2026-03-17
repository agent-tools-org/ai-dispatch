// Hook subcommands for lightweight Claude Code integration.
// Exports: session_start; depends on crate::project and crate::team.

use anyhow::Result;

use crate::project::{self, ProjectConfig};
use crate::team::TeamConfig;

const BASE_TEXT: &str = "[aid] ai-dispatch is installed for multi-agent orchestration. Key commands:
- Dispatch: aid run <agent> \"<prompt>\" [--worktree <branch>]
- Monitor:  aid watch --tui (dashboard) | aid watch --quiet <id> (blocking)
- Review:   aid show <id> --diff | aid board
- Batch:    aid batch <file> --parallel";

pub fn session_start() -> Result<()> {
    let project = project::detect_project();
    let team = project
        .as_ref()
        .and_then(|config| config.team.as_deref())
        .and_then(crate::team::resolve_team);
    println!("{}", render_session_start(project.as_ref(), team.as_ref()));
    Ok(())
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
    use super::render_session_start;
    use crate::project::{ProjectAgents, ProjectBudget, ProjectConfig};
    use crate::team::TeamConfig;
    use std::collections::HashMap;

    #[test]
    fn renders_base_text_without_project() {
        let rendered = render_session_start(None, None);
        assert!(rendered.contains("[aid] ai-dispatch is installed"));
        assert!(!rendered.contains("Project:"));
        assert!(!rendered.contains("Rules:"));
    }

    #[test]
    fn renders_project_and_combined_rule_count() {
        let project = ProjectConfig {
            id: "ai-dispatch".to_string(),
            profile: Some("standard".to_string()),
            max_task_cost: None,
            team: Some("dev".to_string()),
            verify: None,
            language: None,
            rules: vec!["project rule".to_string()],
            budget: ProjectBudget::default(),
            agents: ProjectAgents::default(),
        };
        let team = TeamConfig {
            id: "dev".to_string(),
            display_name: "Dev".to_string(),
            description: String::new(),
            preferred_agents: vec![],
            default_agent: None,
            overrides: HashMap::new(),
            rules: vec!["team rule 1".to_string(), "team rule 2".to_string()],
        };

        let rendered = render_session_start(Some(&project), Some(&team));

        assert!(rendered.contains("Project: ai-dispatch (profile: standard, team: dev)"));
        assert!(rendered.contains("Rules: 3 rule(s)"));
    }
}
