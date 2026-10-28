// Extracted configuration regression tests.
// Deps: parent module and isolated test fixtures.

use super::*;

use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn knowledge_dir_for(team_id: &str) -> PathBuf {
    teams_dir().join(team_id)
}

fn write_team(dir: &Path, file: &str, contents: &str) {
    fs::write(dir.join(file), contents).unwrap();
}

fn sample_team_toml(id: &str) -> String {
    format!(
        r#"[team]
id = "{id}"
display_name = "{id} team"
preferred_agents = ["codex", "opencode"]
"#,
    )
}

#[test]
fn empty_dir_returns_empty() {
    let dir = TempDir::new().unwrap();
    assert!(load_from_dir(&dir.path().to_path_buf()).is_empty());
}

#[test]
fn loads_valid_toml() {
    let dir = TempDir::new().unwrap();
    write_team(dir.path(), "dev.toml", &sample_team_toml("dev"));
    let map = load_from_dir(&dir.path().to_path_buf());
    assert!(map.contains_key("dev"));
    assert_eq!(map["dev"].preferred_agents, vec!["codex", "opencode"]);
}

#[test]
fn skips_invalid_toml() {
    let dir = TempDir::new().unwrap();
    write_team(dir.path(), "bad.toml", "not = valid = toml");
    assert!(load_from_dir(&dir.path().to_path_buf()).is_empty());
}

#[test]
fn parses_full_team_with_overrides() {
    let toml_data = r#"
        [team]
        id = "dev"
        display_name = "Development Team"
        description = "Feature development"
        preferred_agents = ["codex", "opencode", "kilo"]
        default_agent = "codex"

        [team.overrides.opencode]
        simple_edit = 10
        debugging = 6

        [team.overrides.kilo]
        simple_edit = 9
    "#;
    let config = parse_team(toml_data).unwrap();
    assert_eq!(config.id, "dev");
    assert_eq!(config.preferred_agents.len(), 3);
    assert_eq!(config.default_agent, Some("codex".to_string()));
    assert_eq!(config.overrides.len(), 2);
    assert_eq!(config.overrides["opencode"].simple_edit, Some(10));
    assert_eq!(config.overrides["kilo"].simple_edit, Some(9));
}

#[test]
fn parses_team_with_toolbox() {
    let toml_data = r#"
        [team]
        id = "dev"
        display_name = "Dev Team"
        preferred_agents = ["codex"]

        [team.toolbox]
        tools = ["lint-check", "test-runner"]
        auto_inject = ["lint-check"]
    "#;
    let config = parse_team(toml_data).unwrap();
    assert_eq!(config.toolbox.tools, vec!["lint-check", "test-runner"]);
    assert_eq!(config.toolbox.auto_inject, vec!["lint-check"]);
}

#[test]
fn toolbox_defaults_to_empty() {
    let config = parse_team(&sample_team_toml("dev")).unwrap();
    assert!(config.toolbox.tools.is_empty());
    assert!(config.toolbox.auto_inject.is_empty());
}

#[test]
fn list_returns_sorted() {
    let dir = TempDir::new().unwrap();
    write_team(dir.path(), "b.toml", &sample_team_toml("b"));
    write_team(dir.path(), "a.toml", &sample_team_toml("a"));
    let map = load_from_dir(&dir.path().to_path_buf());
    let mut teams: Vec<_> = map.into_values().collect();
    teams.sort_by(|a, b| a.id.cmp(&b.id));
    let ids: Vec<_> = teams.iter().map(|t| t.id.as_str()).collect();
    assert_eq!(ids, vec!["a", "b"]);
}

#[test]
fn read_knowledge_entries_parses_markdown() {
    let _dir = crate::support::test_env::TestHome::new();
    let team_id = "alpha";
    let base = knowledge_dir_for(team_id);
    fs::create_dir_all(base.join("knowledge")).unwrap();
    fs::write(
        base.join("KNOWLEDGE.md"),
        "- [Topic A](knowledge/guide.md) — Useful guide\n- [Topic B] — General note\n",
    )
    .unwrap();
    fs::write(base.join("knowledge/guide.md"), "Guide content\n").unwrap();

    let entries = read_knowledge_entries(team_id);
    assert_eq!(entries.len(), 2);
    let guide = entries
        .iter()
        .find(|entry| entry.topic == "Topic A")
        .unwrap();
    assert_eq!(guide.path.as_deref(), Some("knowledge/guide.md"));
    assert_eq!(guide.description, "Useful guide");
    assert_eq!(guide.content.as_deref(), Some("Guide content"));
    let note = entries
        .iter()
        .find(|entry| entry.topic == "Topic B")
        .unwrap();
    assert!(note.path.is_none());
    assert!(note.content.is_none());
}
