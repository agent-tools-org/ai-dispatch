// Explicit run/batch model-default regressions using isolated AID homes.
// Exercises dry-run dispatch output without launching an external agent.
// Deps: common subprocess helpers and tempfile; CODEX_HOME points at an empty dir.
// agy meters per quota pool, so an unpinned agy run gets a checked model at agy's
// own default effort (medium) instead of the bare CLI default.

mod common;
use common::aid_cmd_in;
use tempfile::TempDir;

fn assert_default_output(output: std::process::Output, codex: usize, agy: usize) {
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(output.status.success(), "{stderr}");
    assert_eq!(stderr.matches("source: CLI default (no -m)").count(), codex, "{stderr}");
    let pinned = stderr.lines().filter(|line| line.contains("source: checked family route"));
    let pinned: Vec<&str> = pinned.collect();
    assert_eq!(pinned.len(), agy, "{stderr}");
    assert!(pinned.iter().all(|line| line.contains("-medium;")), "{stderr}");
    assert!(!stderr.contains("exhausted"), "{stderr}");
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(!stdout.contains("gpt-5.6-luna") && !stdout.contains("gpt-5.6-sol"), "{stdout}");
}

#[test]
fn run_standard_and_premium_use_cli_default_without_config() {
    for agent in ["codex", "agy"] {
        for (difficulty, budget) in [
            ("moderate", "standard"), ("complex", "premium"), ("simple", "standard"),
        ] {
            let home = TempDir::new().expect("isolated home");
            let output = aid_cmd_in(home.path()).env("CODEX_HOME", home.path()).args([
                "run", agent, "Refactor validation", "--no-hint", "--no-skill", "--dry-run",
                "--difficulty", difficulty, "--budget", budget,
                "--urgency", "normal", "--rigor", "standard",
            ]).output().expect("run preview");
            let (codex, agy) = if agent == "agy" { (0, 1) } else { (1, 0) };
            assert_default_output(output, codex, agy);
        }
    }
}

#[test]
fn batch_standard_and_premium_use_cli_default_without_config() {
    let home = TempDir::new().expect("isolated home");
    let batch_path = home.path().join("batch.toml");
    let mut batch = String::new();
    for agent in ["codex", "agy"] {
        for budget in ["standard", "premium"] {
            batch.push_str(&format!(
                "[[tasks]]\nagent = '{agent}'\nprompt = 'Refactor validation'\n\
                 difficulty = 'moderate'\nbudget = '{budget}'\nurgency = 'normal'\n\
                 rigor = 'standard'\nno_skill = true\n",
            ));
        }
    }
    std::fs::write(&batch_path, batch).expect("batch file");
    let output = aid_cmd_in(home.path()).env("CODEX_HOME", home.path()).arg("batch").arg(&batch_path)
        .arg("--dry-run").output().expect("batch preview");
    assert_default_output(output, 2, 2);
}
