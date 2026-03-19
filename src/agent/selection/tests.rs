// Tests for agent auto-selection — classifier accuracy, history weighting, budget mode, team overrides.
// Deps: super::*, classifier, Store, TeamConfig

use super::select_agent_from;
use super::selection_scoring::{CandidateContext, score_for};
use crate::agent::RunOpts;
use crate::agent::classifier;
use crate::paths::{self, AidHomeGuard};
use crate::store::Store;
use crate::team::{CapabilityOverrides, TeamConfig};
use crate::types::AgentKind;
use std::collections::HashMap;
use std::fs;
use rusqlite::params;
use tempfile::TempDir;

fn opts(dir: Option<&str>, budget: bool) -> RunOpts {
    RunOpts {
        dir: dir.map(|s| s.to_string()), output: None, model: None,
        budget, read_only: false, context_files: vec![], session_id: None,
        env: None, env_forward: None,
    }
}
fn isolated() -> (TempDir, AidHomeGuard) {
    let temp = TempDir::new().unwrap();
    let guard = AidHomeGuard::set(temp.path());
    std::fs::create_dir_all(paths::aid_dir()).ok();
    (temp, guard)
}
fn select(prompt: &str, dir: &[&str], available: &[AgentKind]) -> (String, String) {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    select_agent_from(prompt, &opts(dir.first().copied(), false), available, &store, None)
}
fn all() -> [AgentKind; 5] {
    [AgentKind::Gemini, AgentKind::OpenCode, AgentKind::Kilo,
     AgentKind::Cursor, AgentKind::Codex]
}
fn insert_task(store: &Store, id: &str, agent: AgentKind, status: &str, cost: Option<f64>) {
    let conn = store.db();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![id, agent.as_str(), "history", status, "2026-03-15T00:00:00Z", cost],
    )
    .unwrap();
}
fn insert_task_with_prompt(store: &Store, id: &str, agent: AgentKind, status: &str, prompt: &str) {
    let conn = store.db();
    conn.execute(
        "INSERT INTO tasks (id, agent, prompt, status, created_at, cost_usd) VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
        params![
            id,
            agent.as_str(),
            prompt,
            status,
            "2026-03-15T00:00:00Z",
            Option::<f64>::None
        ],
    )
    .unwrap();
}
fn seed_history(store: &Store, agent: AgentKind, successes: usize, failures: usize) {
    for i in 0..successes {
        let id = format!("history-{}-s-{}", agent.as_str(), i);
        insert_task(store, &id, agent, "done", None);
    }
    for i in 0..failures {
        let id = format!("history-{}-f-{}", agent.as_str(), i);
        insert_task(store, &id, agent, "failed", None);
    }
}

#[test]
fn research_tasks_go_to_gemini() {
    let (kind, reason) = select(
        "Explain the authentication flow and compare the docs?", &[], &all(),
    );
    assert_eq!(kind, AgentKind::Gemini.as_str());
    assert!(reason.contains("research"));
    assert!(reason.contains("gemini"));
}
#[test]
fn simple_edits_go_to_opencode() {
    let (kind, reason) = select(
        "rename src/types.rs field name to task_name", &[], &all(),
    );
    assert_eq!(kind, AgentKind::OpenCode.as_str());
    assert!(reason.contains("simple-edit"));
}
#[test]
fn frontend_tasks_go_to_cursor() {
    let (kind, reason) = select(
        "Create a responsive React component layout for the settings UI",
        &["web/app.tsx"], &all(),
    );
    assert_eq!(kind, AgentKind::Cursor.as_str());
    assert!(reason.contains("frontend"));
}

#[test]
fn custom_agent_scores_are_considered() {
    let (_temp, _guard) = isolated();
    let agents_dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("researcher.toml"),
        r#"[agent]
id = "researcher"
display_name = "Researcher Agent"
command = "true"
[agent.capabilities]
research = 12
"#,
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let (kind, _) = select_agent_from(
        "Explain the authentication flow and compare the docs?",
        &opts(None, false), &all(), &store, None,
    );
    assert_eq!(kind, "researcher");
}

#[test]
fn custom_agent_strengths_boost_score() {
    let (_temp, _guard) = isolated();
    let agents_dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("strengthy.toml"),
        r#"[agent]
id = "strengthy"
display_name = "Strengthy Agent"
command = "true"
strengths = ["research"]

[agent.capabilities]
research = 6
"#,
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let (kind, _) = select_agent_from(
        "Explain the authentication flow and compare the docs?",
        &opts(None, false), &all(), &store, None,
    );
    assert_eq!(kind, "strengthy");
}

#[test]
fn custom_strengths_not_matching_category() {
    let (_temp, _guard) = isolated();
    let agents_dir = paths::aid_dir().join("agents");
    fs::create_dir_all(&agents_dir).unwrap();
    fs::write(
        agents_dir.join("docy.toml"),
        r#"[agent]
id = "docy"
display_name = "Doc Agent"
command = "true"
strengths = ["documentation"]

[agent.capabilities]
research = 6
"#,
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let (kind, _) = select_agent_from(
        "Explain the authentication flow and compare the docs?",
        &opts(None, false), &all(), &store, None,
    );
    assert_eq!(kind, AgentKind::Gemini.as_str());
}
#[test]
fn complex_tasks_go_to_codex() {
    let prompt = format!(
        "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. {}",
        "Add validation coverage and refactor the task dispatch flow. ".repeat(12)
    );
    let (kind, reason) = select(&prompt, &["src"], &all());
    assert_eq!(kind, AgentKind::Codex.as_str());
    assert!(reason.contains("complex-impl"));
    assert!(reason.contains("codex"));
}
#[test]
fn unavailable_primary_agent_falls_back_to_next_best() {
    let (kind, reason) = select(
        "rename src/types.rs field name to task_name",
        &[], &[AgentKind::Gemini, AgentKind::Codex],
    );
    assert_eq!(kind, AgentKind::Codex.as_str());
    assert!(reason.contains("simple-edit"));
    assert!(reason.contains("opencode unavailable"));
}
#[test]
fn budget_mode_avoids_codex_for_complex_tasks() {
    let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    // Seed cost data: codex is expensive, cursor is cheap
    for i in 0..5 {
        insert_task(&store, &format!("cost-codex-{i}"), AgentKind::Codex, "done", Some(2.50));
        insert_task(&store, &format!("cost-cursor-{i}"), AgentKind::Cursor, "done", Some(0.10));
    }
    let (kind, reason) = select_agent_from(prompt, &opts(Some("src"), true), &all(), &store, None);
    assert_ne!(kind, AgentKind::Codex.as_str());
    assert!(reason.contains("budget"));
}
#[test]
fn budget_mode_prefers_kilo_for_simple_edits() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let (kind, reason) = select_agent_from(
        "rename src/types.rs field name",
        &opts(Some("src"), true), &[AgentKind::Kilo, AgentKind::Codex], &store, None,
    );
    assert_eq!(kind, AgentKind::Kilo.as_str());
    assert!(reason.contains("budget"));
}
#[test]
fn history_penalty_for_low_success_rate() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Codex, 5, 5);
    let (kind, _) = select_agent_from(
        "add type annotation to field",
        &opts(Some("src"), false), &[AgentKind::OpenCode, AgentKind::Codex], &store, None,
    );
    assert_eq!(kind, AgentKind::OpenCode.as_str());
}
#[test]
fn history_penalty_overrides_default_codex_selection() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Codex, 5, 5);
    let (kind, _) = select_agent_from(
        "some random task", &opts(None, false),
        &[AgentKind::Codex, AgentKind::Gemini], &store, None,
    );
    assert_eq!(kind, AgentKind::Gemini.as_str());
}
#[test]
fn history_bonus_for_high_success_rate() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Codex, 10, 0);
    let (kind, _) = select_agent_from(
        "implement auth flow",
        &opts(None, false), &all(), &store, None,
    );
    assert_eq!(kind, AgentKind::Codex.as_str());
}
#[test]
fn history_prefers_high_success_rate_over_base_score() {
    let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Codex, 6, 4);
    seed_history(&store, AgentKind::Cursor, 19, 1);
    let (kind, _) = select_agent_from(
        prompt, &opts(None, false), &[AgentKind::Cursor, AgentKind::Codex], &store, None,
    );
    assert_eq!(kind, AgentKind::Cursor.as_str());
}
#[test]
fn history_ignored_for_low_task_count() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Gemini, 1, 2);
    let (kind, _) = select_agent_from(
        "research: what is MVP?", &opts(None, false), &all(), &store, None,
    );
    assert_eq!(kind, AgentKind::Gemini.as_str());
}
#[test]
fn history_appears_in_reason() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    seed_history(&store, AgentKind::Gemini, 8, 2);
    let (_, reason) = select_agent_from("research: what is MVP?", &opts(None, false), &all(), &store, None);
    assert!(reason.contains("80% success"));
}
#[test]
fn similar_tasks_hint_appended() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let prompt = "Add routing hints for agent selection";
    for i in 0..3 {
        insert_task_with_prompt(
            &store,
            &format!("hint-done-{i}"),
            AgentKind::Codex,
            "done",
            "Implement routing hints for agent selection",
        );
    }
    insert_task_with_prompt(
        &store,
        "hint-fail",
        AgentKind::Codex,
        "failed",
        "Implement routing hints for agent selection",
    );
    let (_, reason) = select_agent_from(prompt, &opts(None, false), &all(), &store, None);
    assert!(reason.contains("similar tasks"));
    assert!(reason.contains("codex 3/4 success"));
}
#[test]
fn similar_tasks_hint_absent_without_history() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let (_, reason) = select_agent_from(
        "Add routing hints for agent selection",
        &opts(None, false),
        &all(),
        &store,
        None,
    );
    assert!(!reason.contains("similar tasks"));
}

#[test]
fn team_override_boosts_score() {
    let prompt = "rename src/types.rs field name to task_name";
    let normalized = prompt.trim().to_lowercase();
    let profile = classifier::classify(
        prompt,
        classifier::count_file_mentions(&normalized),
        prompt.chars().count(),
    );
    let history_map: HashMap<AgentKind, (f64, usize)> = HashMap::new();
    let avg_cost_map: HashMap<AgentKind, f64> = HashMap::new();
    let base_context = CandidateContext {
        profile: &profile,
        team: None,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default: None,
        budget: false,
    };
    let mut overrides = HashMap::new();
    overrides.insert(
        "opencode".to_string(),
        CapabilityOverrides {
            simple_edit: Some(10),
            ..Default::default()
        },
    );
    let team = TeamConfig {
        id: "override".to_string(),
        display_name: "Override".to_string(),
        description: String::new(),
        preferred_agents: vec![],
        default_agent: None,
        overrides,
        rules: vec![],
        toolbox: Default::default(),
    };
    let team_context = CandidateContext {
        profile: &profile,
        team: Some(&team),
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default: None,
        budget: false,
    };
    let base_score = score_for(&base_context, AgentKind::OpenCode);
    let overridden_score = score_for(&team_context, AgentKind::OpenCode);
    assert!(overridden_score > base_score);
}

#[test]
fn team_preferred_agents_boost() {
    let prompt = "rename src/types.rs field name to task_name";
    let normalized = prompt.trim().to_lowercase();
    let profile = classifier::classify(
        prompt,
        classifier::count_file_mentions(&normalized),
        prompt.chars().count(),
    );
    let history_map: HashMap<AgentKind, (f64, usize)> = HashMap::new();
    let avg_cost_map: HashMap<AgentKind, f64> = HashMap::new();
    let base_context = CandidateContext {
        profile: &profile,
        team: None,
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default: None,
        budget: false,
    };
    let team = TeamConfig {
        id: "preferred".to_string(),
        display_name: "Preferred".to_string(),
        description: String::new(),
        preferred_agents: vec!["kilo".to_string()],
        default_agent: None,
        overrides: HashMap::new(),
        rules: vec![],
        toolbox: Default::default(),
    };
    let preferred_context = CandidateContext {
        profile: &profile,
        team: Some(&team),
        history_map: &history_map,
        avg_cost_map: &avg_cost_map,
        team_default: None,
        budget: false,
    };
    let base_score = score_for(&base_context, AgentKind::Kilo);
    let boosted_score = score_for(&preferred_context, AgentKind::Kilo);
    assert!((boosted_score - base_score - 3.0).abs() < 1e-6);
}

#[test]
fn team_default_agent_tiebreaker() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let mut overrides = HashMap::new();
    overrides.insert(
        "cursor".to_string(),
        CapabilityOverrides {
            simple_edit: Some(5),
            ..Default::default()
        },
    );
    overrides.insert(
        "codex".to_string(),
        CapabilityOverrides {
            simple_edit: Some(5),
            ..Default::default()
        },
    );
    let team = TeamConfig {
        id: "default".to_string(),
        display_name: "Default".to_string(),
        description: String::new(),
        preferred_agents: vec![],
        default_agent: Some("cursor".to_string()),
        overrides,
        rules: vec![],
        toolbox: Default::default(),
    };
    let (kind, _) = select_agent_from(
        "rename src/types.rs field name to task_name",
        &opts(None, false),
        &[AgentKind::Cursor, AgentKind::Codex],
        &store,
        Some(&team),
    );
    assert_eq!(kind, AgentKind::Cursor.as_str());
}

#[test]
fn team_does_not_block_non_preferred() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let team = TeamConfig {
        id: "preferred".to_string(),
        display_name: "Preferred".to_string(),
        description: String::new(),
        preferred_agents: vec!["kilo".to_string()],
        default_agent: None,
        overrides: HashMap::new(),
        rules: vec![],
        toolbox: Default::default(),
    };
    let prompt = "Implement a retry-aware test suite across src/main.rs and src/cmd/run.rs. Add validation coverage.";
    let (kind, _) = select_agent_from(
        prompt,
        &opts(Some("src"), false),
        &[AgentKind::Codex, AgentKind::Cursor],
        &store,
        Some(&team),
    );
    assert_eq!(kind, AgentKind::Codex.as_str());
}

#[test]
fn team_override_overrides_base_score() {
    let (_temp, _guard) = isolated();
    let store = Store::open_memory().unwrap();
    let mut overrides = HashMap::new();
    overrides.insert(
        "gemini".to_string(),
        CapabilityOverrides {
            simple_edit: Some(10),
            ..Default::default()
        },
    );
    let team = TeamConfig {
        id: "override-gemini".to_string(),
        display_name: "Override Gemini".to_string(),
        description: String::new(),
        preferred_agents: vec![],
        default_agent: None,
        overrides,
        rules: vec![],
        toolbox: Default::default(),
    };
    let (kind, _) = select_agent_from(
        "rename src/types.rs field name to task_name",
        &opts(None, false),
        &[AgentKind::Gemini, AgentKind::OpenCode],
        &store,
        Some(&team),
    );
    assert_eq!(kind, AgentKind::Gemini.as_str());
}

#[test]
fn cost_efficiency_calculates_ratio() {
    let value = super::cost_efficiency(9.0, 1.5);
    assert!((value - 3.6).abs() < 1e-6);
}
