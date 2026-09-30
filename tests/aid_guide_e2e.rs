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
    let missing: Vec<_> = command_paths(aid_home.path(), &[]).into_iter()
        .filter(|command| !command_index.contains(&format!("`aid {command}`"))).collect();
    assert!(missing.is_empty(), "official guide missing commands: {missing:?}");
    for reference in skill_references(&skill) {
        assert!(
            guide_dir.join(reference).is_file(),
            "official guide links missing reference: {reference}"
        );
    }
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

fn command_paths(home: &std::path::Path, path: &[String]) -> Vec<String> {
    let help = aid_cmd_in(home).args(path).arg("--help").output().unwrap();
    assert!(help.status.success(), "help failed for aid {}", path.join(" "));
    let mut paths = Vec::new();
    for command in public_commands(&String::from_utf8_lossy(&help.stdout)) {
        let mut child = path.to_vec();
        child.push(command);
        paths.push(child.join(" "));
        paths.extend(command_paths(home, &child));
    }
    paths
}

// Pin only refusal, never, and fail-closed contracts that guide agent actions.
const INVARIANTS: &[(&str /* reference file */, &str /* phrase */)] = &[
    ("dispatch.md", "failure aborts with an error naming the directory"),
    ("dispatch.md", "a NeedsHuman hold still blocks"),
    ("dispatch.md", "refused before a task row is created"),
    ("dispatch.md", "is never turned into a report task by prompt wording"),
    ("task-operations.md", "Repeated activity is not itself a stop condition"),
    ("task-operations.md", "`steer` is refused for the one-shot print-mode `agy` and `grok` CLIs"),
    ("task-operations.md", "`respond` is refused for those same one-shot CLIs"),
    ("task-operations.md", "no response signal was written"),
    ("task-operations.md", "`aid show --output` only renders content proven to belong to that task"),
    ("task-operations.md", "recorded directory stay empty and report absence"),
    ("task-operations.md", "If the worker cannot be stopped, the retry is refused"),
    ("dispatch.md", "without launching an agent or writing the task store"),
    ("dispatch.md", "`aid run auto` and batch `agent = \"auto\"` (or an empty agent) are hard errors"),
    ("collaboration.md", "`auto` and empty agent are rejected"),
    ("dispatch.md", "read the cache only and never probe"),
    ("dispatch.md", "never a fixed fallback model"),
    ("dispatch.md", "exact price-feed entry on its vendor's CLI, never from a\nsimilar-name rate"),
    ("dispatch.md", "dispatch beyond depth `2` is refused"),
    ("dispatch.md", "`--bg` is refused"),
    ("task-lifecycle.md", "records durability before reclaiming"),
    ("task-lifecycle.md", "GC never runs repository-wide `git worktree prune`"),
    ("task-lifecycle.md", "A failed listing\nrefuses collection"),
    ("classify.md", "never in argv"),
    ("classify.md", "never a verdict"),
];

#[test]
fn official_guide_preserves_safety_invariants() {
    for &(reference, phrase) in INVARIANTS {
        let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("default-skills/aid-guide/references").join(reference);
        let content = std::fs::read_to_string(path).unwrap();
        assert!(content.contains(phrase), "missing safety invariant in {reference}: {phrase}");
    }
}
