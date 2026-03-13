// Handler for `aid init` — install bundled skills and templates.
// Exports: run().
// Deps: crate::paths, anyhow, std::fs.

use anyhow::Result;
use std::path::Path;

const SKILL_IMPLEMENTER: &str = include_str!("../../default-skills/implementer.md");
const SKILL_RESEARCHER: &str = include_str!("../../default-skills/researcher.md");
const SKILL_CODE_SCOUT: &str = include_str!("../../default-skills/code-scout.md");
const SKILL_DEBUGGER: &str = include_str!("../../default-skills/debugger.md");
const SKILL_TEST_WRITER: &str = include_str!("../../default-skills/test-writer.md");
const TMPL_BUG_FIX: &str = include_str!("../../default-templates/bug-fix.md");
const TMPL_FEATURE: &str = include_str!("../../default-templates/feature.md");
const TMPL_REFACTOR: &str = include_str!("../../default-templates/refactor.md");
const SKILLS: &[(&str, &str)] = &[
    ("implementer.md", SKILL_IMPLEMENTER),
    ("researcher.md", SKILL_RESEARCHER),
    ("code-scout.md", SKILL_CODE_SCOUT),
    ("debugger.md", SKILL_DEBUGGER),
    ("test-writer.md", SKILL_TEST_WRITER),
];
const TEMPLATES: &[(&str, &str)] = &[
    ("bug-fix.md", TMPL_BUG_FIX),
    ("feature.md", TMPL_FEATURE),
    ("refactor.md", TMPL_REFACTOR),
];

pub fn run() -> Result<()> {
    let base = crate::paths::aid_dir();
    let created_skills = write_defaults(&base.join("skills"), "skill", SKILLS)?;
    let created_templates = write_defaults(&base.join("templates"), "template", TEMPLATES)?;
    println!(
        "Initialized {created_skills} skills and {created_templates} templates in {}",
        base.display()
    );
    Ok(())
}

fn write_defaults(dir: &Path, label: &str, files: &[(&str, &str)]) -> Result<usize> {
    std::fs::create_dir_all(dir)?;
    let mut created = 0;
    for (name, content) in files {
        let path = dir.join(name);
        if path.exists() {
            println!("Skipped existing {label}: {}", path.display());
            continue;
        }
        std::fs::write(&path, content)?;
        println!("Created {label}: {}", path.display());
        created += 1;
    }
    Ok(created)
}
