// Regression coverage for project context selected by `aid run --dir`.
// Exercises real Git repositories and reads the complete persisted dry-run prompt.
// Deps: compiled aid binary, tempfile, rusqlite, Git CLI.

use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::TempDir;

mod common;

struct Fixture {
    _root: TempDir,
    home: PathBuf,
    caller: PathBuf,
    target: PathBuf,
    outside: PathBuf,
}

impl Fixture {
    fn new() -> Self {
        let root = TempDir::new().unwrap();
        let home = root.path().join("home");
        let caller = root.path().join("caller");
        let target = root.path().join("target");
        let outside = root.path().join("outside");
        std::fs::create_dir_all(home.join("skills")).unwrap();
        std::fs::create_dir_all(&outside).unwrap();
        for (repo, id) in [(&caller, "caller"), (&target, "target")] {
            write_project(repo, id);
            let output = common::aid_cmd_with_cwd(&home, repo)
                .args(["memory", "add", "fact", &format!("{id}-memory-marker: this project uses a dedicated architecture registry."), "--tier", "critical"])
                .output().unwrap();
            assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
            std::fs::write(home.join(format!("skills/{id}-skill.md")), format!("{id}-methodology-marker")).unwrap();
        }
        Self { _root: root, home, caller, target, outside }
    }

    fn dispatch(&self, dir: Option<&Path>, extra: &[&str]) -> String {
        let mut command = common::aid_cmd_with_cwd(&self.home, &self.caller);
        command.args(["run", "grok", "Inspect project architecture", "--dry-run", "--id", "t-project-context"]);
        if let Some(dir) = dir { command.arg("--dir").arg(dir); }
        let output = command.args(extra).output().unwrap();
        assert!(output.status.success(), "{}", String::from_utf8_lossy(&output.stderr));
        rusqlite::Connection::open(self.home.join("aid.db")).unwrap()
            .query_row("SELECT resolved_prompt FROM tasks WHERE id = 't-project-context'", [], |row| row.get(0))
            .unwrap()
    }
}

fn write_project(repo: &Path, id: &str) {
    std::fs::create_dir_all(repo.join(".aid/knowledge")).unwrap();
    assert!(Command::new("git").args(["init", "--quiet"]).arg(repo).status().unwrap().success());
    std::fs::write(repo.join(".aid/project.toml"), format!(
        "[project]\nid = \"{id}\"\nrules = [\"{id}-rule-marker\"]\nskills = [\"{id}-skill\"]\n"
    )).unwrap();
    std::fs::write(repo.join(".aid/knowledge/KNOWLEDGE.md"), format!(
        "- [Project Architecture](architecture.md) — {id}-knowledge-marker\n"
    )).unwrap();
    std::fs::write(repo.join(".aid/knowledge/architecture.md"), format!("{id}-architecture-marker")).unwrap();
    std::fs::write(repo.join(".aid/state.toml"), format!(
        "last_updated = \"{}\"\n[health]\nrecent_success_rate = 1.0\ntotal_tasks = 1\n\
         [performance]\nagent_success_rates = {{}}\n[context]\nactive_branch = \"{id}-state-marker\"\n",
        chrono::Utc::now().to_rfc3339()
    )).unwrap();
}

fn assert_project(prompt: &str, expected: &str, excluded: &str) {
    for suffix in ["rule-marker", "methodology-marker", "knowledge-marker", "architecture-marker", "state-marker", "memory-marker"] {
        assert!(prompt.contains(&format!("{expected}-{suffix}")), "missing {expected}-{suffix}: {prompt}");
        assert!(!prompt.contains(&format!("{excluded}-{suffix}")), "leaked {excluded}-{suffix}");
    }
    assert!(prompt.contains(&format!("[Project State: {expected}]")));
    assert!(!prompt.contains(&format!("[Project State: {excluded}]")));
}

#[test]
fn explicit_dir_uses_target_rules_skills_state_and_knowledge() {
    let fixture = Fixture::new();
    assert_project(&fixture.dispatch(Some(&fixture.target), &[]), "target", "caller");
}

#[test]
fn relative_subdirectory_uses_target_project_context() {
    let fixture = Fixture::new();
    std::fs::create_dir(fixture.target.join("src")).unwrap();
    assert_project(&fixture.dispatch(Some(Path::new("../target/src")), &[]), "target", "caller");
}

#[test]
fn no_dir_keeps_callers_project_context() {
    let fixture = Fixture::new();
    assert_project(&fixture.dispatch(None, &[]), "caller", "target");
}

#[test]
fn outside_git_has_no_project_context() {
    let fixture = Fixture::new();
    let prompt = fixture.dispatch(Some(&fixture.outside), &[]);
    for marker in ["caller-", "target-", "[Project State:", "<aid-project-rules>"] {
        assert!(!prompt.contains(marker), "leaked {marker}: {prompt}");
    }
}

#[test]
fn explicit_no_skill_overrides_target_project_default() {
    let fixture = Fixture::new();
    let prompt = fixture.dispatch(Some(&fixture.target), &["--no-skill"]);
    assert!(prompt.contains("target-rule-marker"));
    assert!(!prompt.contains("methodology-marker"));
}

#[test]
fn explicit_skill_overrides_target_project_default() {
    let fixture = Fixture::new();
    let prompt = fixture.dispatch(Some(&fixture.target), &["--skill", "caller-skill"]);
    assert!(prompt.contains("target-rule-marker"));
    assert!(prompt.contains("caller-methodology-marker"));
    assert!(!prompt.contains("target-methodology-marker"));
}

#[test]
fn repo_without_project_config_keeps_its_scoped_memories() {
    let fixture = Fixture::new();
    std::fs::remove_file(fixture.target.join(".aid/project.toml")).unwrap();
    let prompt = fixture.dispatch(Some(&fixture.target), &[]);
    assert!(prompt.contains("target-memory-marker"));
    assert!(!prompt.contains("caller-memory-marker"));
    assert!(!prompt.contains("<aid-project-rules>"));
}
