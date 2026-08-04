// E2E coverage for the release-managed official AID guide skill.
// Verifies installation, reference integrity, and public-command coverage.
// Deps: compiled aid binary and tempfile.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

#[test]
fn official_guide_covers_every_public_command() {
    let aid_home = TempDir::new().unwrap();
    let init = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();
    assert!(init.status.success());
    let guide_dir = aid_home.path().join("skills/aid-guide");
    let skill = std::fs::read_to_string(guide_dir.join("SKILL.md")).unwrap();
    let command_index =
        std::fs::read_to_string(guide_dir.join("references/command-index.md")).unwrap();
    let help = aid_cmd_in(aid_home.path()).arg("--help").output().unwrap();
    assert!(help.status.success());

    for command in public_commands(&String::from_utf8_lossy(&help.stdout)) {
        let documented = format!("`aid {command}`");
        assert!(
            command_index.contains(&documented),
            "official guide does not document public command: {command}"
        );
    }
    for reference in skill_references(&skill) {
        assert!(
            guide_dir.join(reference).is_file(),
            "official guide links missing reference: {reference}"
        );
    }
}

#[test]
fn official_guide_documents_prompt_only_audit_dispatch() {
    let dispatch = include_str!("../default-skills/aid-guide/references/dispatch.md");

    assert!(dispatch.contains("`read-only audit`"));
    assert!(dispatch.contains("`read-only comparative audit`"));
    assert!(dispatch.contains("`read-only cross-audit`"));
    assert!(dispatch.contains("`read-only re-audit`"));
    assert!(dispatch.contains("Omit `--read-only`"));
    assert!(dispatch.contains("`add an audit log`"));
    assert!(dispatch.contains("`make changes to the read-only audit logic`"));
    assert!(dispatch.contains("`do not modify` or `without modifying`"));
    assert!(dispatch.contains("independent of dirty-worktree enforcement"));
    assert!(dispatch.contains("`--result-file` controls report formatting and delivery"));
}

#[test]
fn official_guide_documents_sustained_loop_protection() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("persists for at least two minutes"));
    assert!(operations.contains("ten consecutive"));
    assert!(operations.contains("format, or lint activity"));
    assert!(operations.contains("For custom text agents"));
    assert!(operations.contains("structured-agent reasoning narration alone never justifies"));
    assert!(operations.contains("`aid show <task-id> --events`"));
}

fn public_commands(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|line| *line != "Commands:")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|command| *command != "help")
        .map(str::to_string)
        .collect()
}

fn skill_references(skill: &str) -> Vec<&str> {
    skill
        .split('(')
        .filter_map(|part| part.split_once(')'))
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("references/"))
        .collect()
}
