// Tests for folder-aware skill loading and discovery helpers.
// Exports: none.
// Deps: crate::skills, crate::paths, AgentKind, tempfile.

use super::*;

#[test]
fn loads_folder_skill_from_skill_md() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir().join("implementer");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), "# Implementer").unwrap();

    assert_eq!(load_skill("implementer").unwrap(), "# Implementer");
}

#[test]
fn loads_flat_skill_for_backward_compatibility() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("test-writer.md"), "# Test Writer").unwrap();

    assert_eq!(load_skill("test-writer").unwrap(), "# Test Writer");
}

#[test]
fn lists_flat_and_folder_skills() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir();
    std::fs::create_dir_all(dir.join("implementer")).unwrap();
    std::fs::write(dir.join("implementer").join("SKILL.md"), "# Implementer").unwrap();
    std::fs::write(dir.join("reviewer.md"), "# Reviewer").unwrap();

    assert_eq!(list_skills().unwrap(), vec!["implementer", "reviewer"]);
}

#[test]
fn load_skill_rejects_invalid_name() {
    let err = load_skill("../escape").unwrap_err();
    assert!(err.to_string().contains("Invalid skill name"));
}

#[test]
fn loads_general_and_agent_specific_gotchas() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir().join("implementer");
    std::fs::create_dir_all(dir.join("gotchas")).unwrap();
    std::fs::write(dir.join("SKILL.md"), "# Implementer").unwrap();
    std::fs::write(dir.join("gotchas.md"), "general").unwrap();
    std::fs::write(dir.join("gotchas").join("codex.md"), "agent").unwrap();

    assert_eq!(
        load_skill_gotchas("implementer", &AgentKind::Codex),
        Some("general\n\nagent".to_string())
    );
}

#[test]
fn lists_scripts_and_references_for_folder_skill() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir().join("implementer");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::create_dir_all(dir.join("references")).unwrap();
    std::fs::write(dir.join("SKILL.md"), "# Implementer").unwrap();
    std::fs::write(dir.join("scripts").join("b.sh"), "").unwrap();
    std::fs::write(dir.join("scripts").join("a.sh"), "").unwrap();
    std::fs::write(dir.join("references").join("api.md"), "").unwrap();

    assert_eq!(
        list_skill_scripts("implementer"),
        vec![
            dir.join("scripts").join("a.sh").display().to_string(),
            dir.join("scripts").join("b.sh").display().to_string(),
        ]
    );
    assert_eq!(
        list_skill_references("implementer"),
        vec![dir.join("references").join("api.md").display().to_string()]
    );
}

#[test]
fn handles_skill_folder_without_optional_content() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir().join("implementer");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("SKILL.md"), "# Implementer").unwrap();

    assert_eq!(load_skill("implementer").unwrap(), "# Implementer");
    assert_eq!(load_skill_gotchas("implementer", &AgentKind::Codex), None);
    assert!(list_skill_scripts("implementer").is_empty());
    assert!(list_skill_references("implementer").is_empty());
}

#[test]
fn measure_skill_tokens_includes_gotchas_and_script_listing() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir().join("implementer");
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::write(dir.join("SKILL.md"), "abcd").unwrap();
    std::fs::write(dir.join("gotchas.md"), "efgh").unwrap();
    std::fs::write(dir.join("scripts").join("tool.sh"), "").unwrap();

    let (_, tokens) = measure_skill_tokens("implementer").unwrap();
    let expected = estimate_tokens(
        &format!(
            "abcd\n\nefgh\n\n{}",
            dir.join("scripts").join("tool.sh").display()
        )
    );
    assert_eq!(tokens, expected);
}

#[test]
fn auto_skills_returns_agent_defaults_when_installed() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();
    std::fs::write(dir.join("researcher.md"), "# Researcher").unwrap();

    assert_eq!(auto_skills(&AgentKind::Codex, false), vec!["implementer"]);
    assert_eq!(auto_skills(&AgentKind::OpenCode, false), vec!["implementer"]);
    assert!(auto_skills(&AgentKind::Cursor, true).is_empty());
    assert_eq!(auto_skills(&AgentKind::Gemini, false), vec!["researcher"]);
    assert_eq!(auto_skills(&AgentKind::Kilo, false), vec!["implementer"]);
}

#[test]
fn auto_skills_skips_missing_defaults() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = skills_dir();
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(dir.join("implementer.md"), "# Implementer").unwrap();

    assert!(auto_skills(&AgentKind::Gemini, false).is_empty());
}

#[test]
fn estimate_tokens_uses_length_divided_by_four() {
    assert_eq!(estimate_tokens("abcd"), 1);
    assert_eq!(estimate_tokens("abc"), 0);
    assert_eq!(estimate_tokens(""), 0);
}
