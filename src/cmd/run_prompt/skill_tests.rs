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

    let injected = inject_skill("prompt", &AgentKind::Codex, &["implementer".to_string()]).unwrap();

    assert!(injected.contains("prompt"));
    assert!(injected.contains("--- Gotchas ---\ngeneral\n\nagent"));
    assert!(injected.contains("--- Methodology ---\nmethod"));
    assert!(injected.contains("--- Available Scripts ---"));
    assert!(injected.contains(&format!("- {}", dir.join("scripts").join("check.sh").display())));
    assert!(injected.contains("--- References (read on demand) ---"));
    assert!(injected.contains(&format!("- {}", dir.join("references").join("api.md").display())));
}
