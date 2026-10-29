// Team command tests for creation, deletion, and display.
// Deps: parent command helpers, temporary aid homes.

use super::*;
use std::{fs, path::PathBuf};

fn team_file(name: &str) -> PathBuf {
    team::teams_dir().join(format!("{name}.toml"))
}

fn write_team_file(name: &str, contents: &str) {
    let dir = team::teams_dir();
    fs::create_dir_all(&dir).unwrap();
    fs::write(dir.join(format!("{name}.toml")), contents).unwrap();
}

#[test]
fn create_team_happy_path() {
    let _temp = crate::support::test_env::TestHome::new();
    run_team_command(TeamAction::Create {
        name: "ops".to_string(),
    })
    .unwrap();

    let file = team_file("ops");
    assert!(file.is_file());
    assert!(team::knowledge_dir("ops").is_dir());
    let knowledge_index = team::knowledge_index("ops");
    let contents = fs::read_to_string(&knowledge_index).unwrap();
    assert!(contents.contains("# Ops — Team Knowledge"));
    assert!(toolbox::team_tools_dir("ops").is_dir());
    assert!(toolbox::team_tools_dir("ops").join("scripts").is_dir());
}

#[test]
fn create_team_duplicate_name_error() {
    let _temp = crate::support::test_env::TestHome::new();
    run_team_command(TeamAction::Create {
        name: "ops".to_string(),
    })
    .unwrap();
    let err = run_team_command(TeamAction::Create {
        name: "ops".to_string(),
    })
    .unwrap_err();
    assert!(err.to_string().contains("already exists"));
}

#[test]
fn delete_team_happy_path() {
    let _temp = crate::support::test_env::TestHome::new();
    run_team_command(TeamAction::Create {
        name: "removable".to_string(),
    })
    .unwrap();
    let file = team_file("removable");
    assert!(file.is_file());
    run_team_command(TeamAction::Delete {
        name: "removable".to_string(),
    })
    .unwrap();
    assert!(!file.exists());
}

#[test]
fn delete_team_missing_name_error() {
    let _temp = crate::support::test_env::TestHome::new();
    let err = run_team_command(TeamAction::Delete {
        name: "ghost".to_string(),
    })
    .unwrap_err();
    assert!(err.to_string().contains("does not exist"));
}

#[test]
fn list_teams_no_teams() {
    let _temp = crate::support::test_env::TestHome::new();
    run_team_command(TeamAction::List).unwrap();
}

#[test]
fn list_teams_multiple_entries() {
    let _temp = crate::support::test_env::TestHome::new();
    run_team_command(TeamAction::Create {
        name: "alpha".to_string(),
    })
    .unwrap();
    run_team_command(TeamAction::Create {
        name: "beta".to_string(),
    })
    .unwrap();
    run_team_command(TeamAction::List).unwrap();
}

#[test]
fn show_team_with_overrides() {
    let _temp = crate::support::test_env::TestHome::new();
    let contents = r#"
            [team]
            id = "ops"
            display_name = "Operations"
            preferred_agents = ["codex"]
            default_agent = "opencode"
            description = "Ops work"

            [team.overrides.codex]
            simple_edit = 5
            debugging = 3
        "#;
    write_team_file("ops", contents);
    run_team_command(TeamAction::Show {
        name: "ops".to_string(),
    })
    .unwrap();
}

#[test]
fn show_team_without_overrides() {
    let _temp = crate::support::test_env::TestHome::new();
    let contents = r#"
            [team]
            id = "solo"
            display_name = "Solo"
            preferred_agents = []
        "#;
    write_team_file("solo", contents);
    run_team_command(TeamAction::Show {
        name: "solo".to_string(),
    })
    .unwrap();
}

#[test]
fn show_team_not_found_error() {
    let _temp = crate::support::test_env::TestHome::new();
    let err = run_team_command(TeamAction::Show {
        name: "missing".to_string(),
    })
    .unwrap_err();
    assert!(err.to_string().contains("not found"));
}

#[test]
fn title_case_variants() {
    assert_eq!(title_case("my-team"), "My Team");
    assert_eq!(title_case("dev_ops"), "Dev Ops");
    assert_eq!(title_case("solo"), "Solo");
    assert_eq!(title_case(""), "");
}
