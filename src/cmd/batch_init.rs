// Generate a batch TOML template with all available fields.
// Exports: init(); depends on crate::project for auto-detecting project defaults.

use anyhow::Result;

/// Generate a batch TOML template file, pre-filled with project defaults.
pub fn init(output_path: Option<&str>) -> Result<()> {
    let path = output_path.unwrap_or("tasks.toml");
    if std::path::Path::new(path).exists() {
        anyhow::bail!("{path} already exists — use -o <file> to choose a different name");
    }

    let (dir, team, verify, language) = detect_project_defaults();
    let template = render_template(&dir, &team, &verify, &language);

    std::fs::write(path, &template)?;
    println!("[aid] Created {path}");
    println!("[aid] Edit the [[task]] entries, then run: aid batch {path} --parallel");
    Ok(())
}

fn detect_project_defaults() -> (String, String, String, String) {
    let project = crate::project::detect_project();
    match project {
        Some(config) => {
            let dir = ".".to_string();
            let team = config.team.unwrap_or_default();
            let verify = config.verify.unwrap_or_default();
            let language = config.language.unwrap_or_default();
            (dir, team, verify, language)
        }
        None => (String::new(), String::new(), String::new(), String::new()),
    }
}

fn render_template(dir: &str, team: &str, verify: &str, language: &str) -> String {
    let mut lines = vec![
        "# Batch task file for aid".to_string(),
        "# Docs: aid batch --help".to_string(),
        "# All fields below also work as task-level overrides.".to_string(),
        String::new(),
    ];

    // [defaults] section — core
    lines.push("[defaults]".to_string());
    if dir.is_empty() {
        lines.push("# dir = \".\"                    # Working directory".to_string());
    } else {
        lines.push(format!("dir = \"{dir}\""));
    }
    lines.push("# agent = \"codex\"              # Default agent for all tasks".to_string());
    if team.is_empty() {
        lines.push("# team = \"dev\"                 # Team knowledge context".to_string());
    } else {
        lines.push(format!("team = \"{team}\""));
    }
    lines.push("# model = \"o3\"                 # Model override".to_string());
    if verify.is_empty() {
        lines.push("# verify = \"cargo check\"       # Auto-verify on completion".to_string());
    } else {
        lines.push(format!("verify = \"{verify}\""));
    }
    lines.push("# fallback = \"cursor,opencode\" # Comma-separated fallback agents".to_string());
    lines.push(String::new());

    // [defaults] — context & constraints
    lines.push("# --- Context & constraints ---".to_string());
    lines.push("# context = [\"src/types.rs\"]    # Files to inject as context".to_string());
    lines.push("# skills = [\"implementer\"]      # Methodology skills".to_string());
    lines.push("# no_skill = false              # Disable skill injection".to_string());
    lines.push("# scope = [\"src/\"]              # Restrict file access".to_string());
    lines.push("# checklist = [\"must compile\", \"must have tests\"]  # Quality checklist".to_string());
    lines.push("# read_only = false             # Read-only mode".to_string());
    lines.push("# sandbox = false               # Run agent in sandbox mode".to_string());
    lines.push("# budget = false                # Use budget-optimized model".to_string());
    lines.push(String::new());

    // [defaults] — execution
    lines.push("# --- Execution ---".to_string());
    lines.push("# max_duration_mins = 30        # Per-task hard timeout (minutes)".to_string());
    lines.push("# retry = 1                     # Retry failed runs N times".to_string());
    lines.push("# idle_timeout = 120            # Kill if idle for N seconds".to_string());
    lines.push("# judge = true                  # AI judge evaluates output quality".to_string());
    lines.push("# peer_review = \"gemini\"       # Run a second-agent review".to_string());
    lines.push("# best_of = 3                   # Run N copies, pick best".to_string());
    lines.push("# metric = \"cargo test\"        # Best-of scoring command".to_string());
    lines.push("# container = \"node:20\"         # Run agent inside container".to_string());
    lines.push("# on_done = \"notify done\"      # Shell command after completion".to_string());
    lines.push("# hooks = [\"pre:lint\"]           # Hook specs".to_string());
    lines.push(String::new());

    // [defaults] — worktree & grouping
    lines.push("# --- Worktree & grouping ---".to_string());
    lines.push("# worktree_prefix = \"feat/v9\"   # Auto-generate worktree per task".to_string());
    lines.push("# group = \"wg-abc1\"             # Assign all tasks to workgroup".to_string());
    lines.push("# shared_dir = false            # Shared directory for inter-task files".to_string());
    lines.push("# analyze = true                # Warn about overlapping file edits".to_string());
    lines.push(String::new());

    // First task
    lines.push("[[task]]".to_string());
    lines.push("name = \"task-1\"".to_string());
    lines.push("agent = \"codex\"".to_string());
    lines.push("prompt = \"\"\"".to_string());
    if language.is_empty() {
        lines.push("<DESCRIBE_TASK_HERE>".to_string());
    } else {
        lines.push(format!("Implement <FEATURE> in {language}."));
    }
    lines.push("\"\"\"".to_string());
    lines.push("# worktree = \"feat/<BRANCH>\"    # Git worktree for isolation".to_string());
    lines.push("# context = [\"src/types.rs\"]    # Extra context files".to_string());
    lines.push("# context_from = [\"task-0\"]     # Inject output from previous tasks".to_string());
    lines.push("# checklist = [\"no unwrap()\"]    # Quality checklist items".to_string());
    lines.push("# no_skill = true               # Disable skill injection".to_string());
    lines.push("# sandbox = true                # Run in sandbox mode".to_string());
    lines.push("# idle_timeout = 120            # Kill if idle for N seconds".to_string());
    lines.push("# retry = 2                     # Retry the task on failure".to_string());
    lines.push("# peer_review = \"gemini\"       # Run a second-agent review".to_string());
    lines.push("# best_of = 3                   # Run N copies, pick best".to_string());
    lines.push("# metric = \"cargo test\"        # Best-of scoring command".to_string());
    lines.push("# scope = [\"src/parser/\"]       # Restrict file access".to_string());
    lines.push("# depends_on = [\"other-task\"]   # Run after named task(s)".to_string());
    lines.push("# on_done = \"notify done\"      # Shell command after completion".to_string());
    lines.push("# on_success = \"deploy\"         # Trigger conditional task on success".to_string());
    lines.push("# on_fail = \"notify\"            # Trigger conditional task on failure".to_string());
    lines.push("# env = { RUST_LOG = \"debug\" }  # Task-specific env vars".to_string());
    lines.push(String::new());

    // Second task (dependent example)
    lines.push("# [[task]]".to_string());
    lines.push("# name = \"task-2\"".to_string());
    lines.push("# agent = \"opencode\"".to_string());
    lines.push("# prompt = \"<DESCRIBE_TASK_HERE>\"".to_string());
    lines.push("# depends_on = [\"task-1\"]".to_string());
    lines.push("# context_from = [\"task-1\"]     # Use task-1 output as context".to_string());
    lines.push(String::new());

    // Conditional task example
    lines.push("# [[task]]".to_string());
    lines.push("# name = \"deploy\"".to_string());
    lines.push("# conditional = true             # Only runs when triggered".to_string());
    lines.push("# agent = \"codex\"".to_string());
    lines.push("# prompt = \"Run deploy script\"".to_string());
    lines.push(String::new());

    lines.join("\n") + "\n"
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn render_template_with_no_project() {
        let template = render_template("", "", "", "");
        assert!(template.contains("[defaults]"));
        assert!(template.contains("[[task]]"));
        assert!(template.contains("<DESCRIBE_TASK_HERE>"));
        assert!(template.contains("# fallback"));
        assert!(template.contains("# depends_on"));
        // New fields present
        assert!(template.contains("# idle_timeout"));
        assert!(template.contains("# checklist"));
        assert!(template.contains("# context_from"));
        assert!(template.contains("# scope"));
        assert!(template.contains("# conditional"));
        assert!(template.contains("# env ="));
        assert!(template.contains("# sandbox"));
        assert!(template.contains("# retry"));
        assert!(template.contains("# peer_review"));
        assert!(template.contains("# metric"));
        assert!(template.contains("# on_done"));
        assert!(template.contains("# no_skill"));
    }

    #[test]
    fn render_template_with_project_defaults() {
        let template = render_template(".", "dev", "cargo test", "rust");
        assert!(template.contains("dir = \".\""));
        assert!(template.contains("team = \"dev\""));
        assert!(template.contains("verify = \"cargo test\""));
        assert!(template.contains("Implement <FEATURE> in rust."));
        assert!(!template.contains("# dir ="));
        assert!(!template.contains("# team ="));
    }

    #[test]
    fn init_refuses_existing_file() {
        let temp = tempfile::NamedTempFile::new().unwrap();
        let path = temp.path().to_str().unwrap();
        let result = init(Some(path));
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("already exists"));
    }

    #[test]
    fn init_creates_file() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("my-tasks.toml");
        let path_str = path.to_str().unwrap();
        init(Some(path_str)).unwrap();
        let content = std::fs::read_to_string(&path).unwrap();
        assert!(content.contains("[defaults]"));
        assert!(content.contains("[[task]]"));
    }
}
