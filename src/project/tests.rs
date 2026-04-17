// Tests for project config parsing, profiles, and knowledge loading.
// Exports: none; loaded by `project.rs` under `#[cfg(test)]`.
// Deps: super, std::env/fs/path, tempfile.

use super::*;
use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use tempfile::TempDir;

fn write_project(dir: &Path, contents: &str) -> PathBuf {
    let path = dir.join("project.toml");
    fs::write(&path, contents).unwrap();
    path
}

struct TempCwd {
    previous: PathBuf,
}

impl TempCwd {
    fn enter(target: &Path) -> Self {
        let previous = env::current_dir().unwrap();
        env::set_current_dir(target).unwrap();
        Self { previous }
    }
}

impl Drop for TempCwd {
    fn drop(&mut self) {
        env::set_current_dir(&self.previous).unwrap();
    }
}

#[test]
fn parses_minimal_toml() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "alpha"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.id, "alpha");
    assert!(config.rules.is_empty());
}

#[test]
fn profile_expands_standard_defaults() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "beta"
profile = "standard"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.max_task_cost, Some(10.0));
    assert_eq!(config.verify.as_deref(), Some("auto"));
    assert_eq!(config.budget.cost_limit_usd, Some(20.0));
    assert!(config.rules.iter().any(|rule| rule.contains("new functions")));
}

#[test]
fn profile_defaults_respect_explicit_values() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "gamma"
profile = "standard"
max_task_cost = 3.5
verify = "custom verify"
setup = "npm ci"
rules = ["explicit rule"]
budget.window = "4h"
budget.cost_limit_usd = 99.5
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.max_task_cost, Some(3.5));
    assert_eq!(config.verify.as_deref(), Some("custom verify"));
    assert_eq!(config.setup.as_deref(), Some("npm ci"));
    assert!(config.rules.iter().any(|rule| rule == "explicit rule"));
    assert!(config.rules.iter().any(|rule| rule.contains("new functions")));
    assert_eq!(config.budget.cost_limit_usd, Some(99.5));
}

#[test]
fn strict_toml_rejects_unknown_top_level() {
    let dir = TempDir::new().unwrap();
    let contents = r#"
[project]
id = "test"
[budget]
daily_limit = "$50"
"#;
    assert!(load_project(&write_project(dir.path(), contents)).is_err());
}

#[test]
fn budget_shorthand_day() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
budget = "$1000/day"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.budget.cost_limit_usd, Some(1000.0));
    assert_eq!(config.budget.window.as_deref(), Some("daily"));
    assert_eq!(config.budget.budget_shorthand(), Some("$1000/day".to_string()));
}

#[test]
fn budget_shorthand_plain_number() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
budget = "$500"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.budget.cost_limit_usd, Some(500.0));
    assert!(config.budget.window.is_none());
}

#[test]
fn budget_shorthand_month() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
budget = "$2000/month"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.budget.cost_limit_usd, Some(2000.0));
    assert_eq!(config.budget.window.as_deref(), Some("monthly"));
}

#[test]
fn parses_container_image() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
container = "dev:latest"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.container.as_deref(), Some("dev:latest"));
}

#[test]
fn parses_auto_gc_mode() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
aid_gc = "auto"
worktree_prefix = "feat/team"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert!(config.aid_gc_auto());
    assert_eq!(config.worktree_prefix.as_deref(), Some("feat/team"));
}

#[test]
fn gitbutler_mode_round_trips_from_toml() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "test"
gitbutler = "auto"
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.gitbutler.as_deref(), Some("auto"));
    assert_eq!(config.gitbutler_mode(), crate::gitbutler::Mode::Auto);
}

#[test]
fn gitbutler_mode_falls_back_to_off_for_invalid_values() {
    let config = ProjectConfig {
        id: "test".to_string(),
        gitbutler: Some("broken".to_string()),
        ..Default::default()
    };

    assert_eq!(config.gitbutler_mode(), crate::gitbutler::Mode::Off);
}

#[test]
fn audit_auto_reads_top_level_section() {
    let dir = TempDir::new().unwrap();
    let contents = r#"
[project]
id = "test"

[audit]
auto = true
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert!(config.audit_auto());
}

#[test]
fn parses_max_task_cost_from_toml() {
    let dir = TempDir::new().unwrap();
    let contents = r#"[project]
id = "delta"
max_task_cost = 7.25
"#;
    let config = load_project(&write_project(dir.path(), contents)).unwrap();
    assert_eq!(config.max_task_cost, Some(7.25));
}

#[test]
fn profile_sets_default_max_task_costs() {
    let dir = TempDir::new().unwrap();

    let hobby = load_project(&write_project(
        dir.path(),
        r#"[project]
id = "hobby"
profile = "hobby"
"#,
    ))
    .unwrap();
    assert_eq!(hobby.max_task_cost, Some(2.0));

    let standard = load_project(&write_project(
        dir.path(),
        r#"[project]
id = "standard"
profile = "standard"
"#,
    ))
    .unwrap();
    assert_eq!(standard.max_task_cost, Some(10.0));

    let production = load_project(&write_project(
        dir.path(),
        r#"[project]
id = "production"
profile = "production"
"#,
    ))
    .unwrap();
    assert_eq!(production.max_task_cost, Some(25.0));
}

#[test]
fn detect_project_returns_none_outside_git() {
    let dir = TempDir::new().unwrap();
    let _guard = TempCwd::enter(dir.path());
    assert!(detect_project().is_none());
}

#[test]
fn test_read_project_knowledge() {
    let dir = TempDir::new().unwrap();
    let git_root = dir.path();
    fs::create_dir_all(git_root.join(".git")).unwrap();
    let knowledge_dir = project_knowledge_dir(git_root);
    fs::create_dir_all(&knowledge_dir).unwrap();
    fs::write(
        knowledge_dir.join("KNOWLEDGE.md"),
        "- [Guide](guide.md) — Useful knowledge\n- [Note] — Standalone note\n",
    )
    .unwrap();
    fs::write(knowledge_dir.join("guide.md"), "Details\n").unwrap();

    let entries = read_project_knowledge(git_root);
    assert_eq!(entries.len(), 2);
    let guide = entries.into_iter().find(|entry| entry.topic == "Guide").unwrap();
    assert_eq!(guide.path.as_deref(), Some("guide.md"));
    assert_eq!(guide.description, "Useful knowledge");
    assert_eq!(guide.content.as_deref(), Some("Details"));
}
