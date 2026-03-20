// Tests for skill injection formatting in `cmd::run_prompt`.
// Exports: none.
// Deps: run_prompt::inject_skill, crate::paths, AgentKind, tempfile.

use super::inject_skill;
use crate::types::AgentKind;

#[test]
fn inject_skill_includes_gotchas_scripts_and_references_for_folder_skill() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills").join("implementer");
    std::fs::create_dir_all(dir.join("gotchas")).unwrap();
    std::fs::create_dir_all(dir.join("scripts")).unwrap();
    std::fs::create_dir_all(dir.join("references")).unwrap();
    std::fs::write(dir.join("SKILL.md"), "method").unwrap();
    std::fs::write(dir.join("gotchas.md"), "general").unwrap();
    std::fs::write(dir.join("gotchas").join("codex.md"), "agent").unwrap();
    std::fs::write(dir.join("scripts").join("check.sh"), "").unwrap();
    std::fs::write(dir.join("references").join("api.md"), "").unwrap();

    let long_prompt = "x".repeat(250);
    let injected = inject_skill(&long_prompt, &AgentKind::Codex, &["implementer".to_string()], 250).unwrap();

    assert!(injected.contains(&long_prompt));
    assert!(injected.contains("--- Gotchas ---\ngeneral\n\nagent"));
    assert!(injected.contains("--- Methodology ---\nmethod"));
    assert!(injected.contains("--- Available Scripts ---"));
    assert!(injected.contains(&format!("- {}", dir.join("scripts").join("check.sh").display())));
    assert!(injected.contains("--- References (read on demand) ---"));
    assert!(injected.contains(&format!("- {}", dir.join("references").join("api.md").display())));
}

#[test]
fn inject_skill_skips_methodology_for_short_prompts() {
    let temp = tempfile::tempdir().unwrap();
    let _aid_home = crate::paths::AidHomeGuard::set(temp.path());
    let dir = crate::paths::aid_dir().join("skills").join("implementer");
    std::fs::create_dir_all(dir.join("references")).unwrap();
    std::fs::write(dir.join("SKILL.md"), "method").unwrap();
    std::fs::write(dir.join("gotchas.md"), "general gotcha").unwrap();

    let injected = inject_skill("short prompt", &AgentKind::Codex, &["implementer".to_string()], 12).unwrap();

    assert!(injected.contains("short prompt"));
    assert!(!injected.contains("--- Methodology ---"), "methodology should be skipped for short prompts");
    assert!(!injected.contains("--- Gotchas ---"), "gotchas should be skipped for short prompts");
}
