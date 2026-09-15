// Session-start hook tests: fleet line is silent unless a hold or Degraded exists.

use super::{QuotaState, render_agents_status_line, render_session_start};
use crate::project::{ProjectAgents, ProjectBudget, ProjectConfig};
use crate::team::TeamConfig;
use std::collections::HashMap;

fn ok() -> QuotaState {
    QuotaState::Ok {
        used_percent: None,
        stale: false,
    }
}

fn limited(resets: Option<chrono::NaiveDateTime>) -> QuotaState {
    QuotaState::Limited {
        resets,
        used_percent: None,
        stale: false,
        human: None,
    }
}

fn entry(name: &str, state: QuotaState) -> (String, QuotaState) {
    (name.to_string(), state)
}

#[test]
fn agent_line_silent_when_all_ok() {
    let entries = vec![entry("codex", ok()), entry("agy", ok())];
    assert_eq!(render_agents_status_line(&entries), None);
}

#[test]
fn agent_line_formats_limited_fleet() {
    let resets = chrono::NaiveDateTime::parse_from_str("2026-08-05 18:27", "%Y-%m-%d %H:%M")
        .expect("valid datetime");
    let entries = vec![
        entry("codex", limited(Some(resets))),
        entry("agy", ok()),
        entry("opencode", ok()),
    ];
    assert_eq!(
        render_agents_status_line(&entries).as_deref(),
        Some("agents: codex LIMITED (resets 18:27) - agy ok - opencode ok")
    );
}

#[test]
fn agent_line_omits_reset_time_when_unknown() {
    let entries = vec![entry("codex", limited(None)), entry("agy", ok())];
    assert_eq!(
        render_agents_status_line(&entries).as_deref(),
        Some("agents: codex LIMITED - agy ok")
    );
}

#[test]
fn agent_line_shows_needs_human_reason() {
    let reason = "needs human: Error: Your credentials are invalid. Please log in again with `oz login`. — fix, then `aid config clear-limit oz`";
    let entries = vec![
        entry(
            "oz",
            QuotaState::Limited {
                resets: None,
                used_percent: None,
                stale: false,
                human: Some(reason.to_string()),
            },
        ),
        entry("agy", ok()),
    ];
    let expected = format!("agents: oz {reason} - agy ok");
    assert_eq!(
        render_agents_status_line(&entries).as_deref(),
        Some(expected.as_str())
    );
}

#[test]
fn agent_line_shows_partial_and_live_percent() {
    let entries = vec![
        entry(
            "grok",
            QuotaState::Ok {
                used_percent: Some(0.0),
                stale: false,
            },
        ),
        entry(
            "cursor",
            QuotaState::Partial {
                used_percent: None,
                stale: false,
            },
        ),
    ];
    assert_eq!(
        render_agents_status_line(&entries).as_deref(),
        Some("agents: grok ok (0%) - cursor PARTIAL")
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
fn agents_status_line_shows_oz_credentials_hold() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).expect("create aid dir");
    let _fleet = crate::agent::DetectAgentsGuard::set(vec![
        crate::types::AgentKind::Oz,
        crate::types::AgentKind::Antigravity,
    ]);
    crate::rate_limit::mark_rate_limited(
        &crate::types::AgentKind::Oz,
        None,
        "Error: Your credentials are invalid. Please log in again with `oz login`.",
    );
    let line = super::agents_status_line().expect("line when oz is held");
    assert!(
        line.contains("needs human: Error: Your credentials are invalid."),
        "{line}"
    );
    assert!(line.contains("aid config clear-limit oz"), "{line}");
    assert!(!line.contains("oz LIMITED"), "{line}");
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

fn with_codex_probe(
    used: f64,
) -> (
    tempfile::TempDir,
    crate::paths::AidHomeGuard,
    crate::live_quota::CacheDirGuard,
    crate::agent::DetectAgentsGuard,
) {
    let temp = tempfile::tempdir().expect("tempdir");
    let _ = std::fs::create_dir_all(temp.path().join(".aid"));
    let home = crate::paths::AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(crate::paths::aid_dir()).expect("create aid dir");
    let aidbar = temp.path().join("aidbar");
    std::fs::create_dir_all(&aidbar).expect("cache");
    let fetched = chrono::Utc::now().to_rfc3339();
    std::fs::write(
        aidbar.join("codex.json"),
        format!(
            r#"{{"ok":true,"snapshot":{{"provider":"codex","windows":[{{"label":"Weekly","used_percent":{used},"resets_at":"2026-09-19T08:10:00Z"}}],"fetched_at":"{fetched}"}}}}"#
        ),
    )
    .expect("cache");
    let cache = crate::live_quota::CacheDirGuard::set(&aidbar);
    let fleet = crate::agent::DetectAgentsGuard::set(vec![crate::types::AgentKind::Codex]);
    (temp, home, cache, fleet)
}

#[test]
fn hook_line_names_degraded_at_96_and_omits_ok() {
    let _guards = with_codex_probe(96.0);
    let line = super::agents_status_line().expect("degraded fleet line");
    assert_eq!(line, "agents: codex DEGRADED (96%, resets 08:10)");
    assert!(!line.contains(" ok"), "{line}");
}

#[test]
fn hook_line_silent_when_probe_is_20_percent() {
    let _guards = with_codex_probe(20.0);
    assert_eq!(super::agents_status_line(), None);
}
