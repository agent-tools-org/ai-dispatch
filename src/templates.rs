// Prompt templates injected into agent prompts.
// Shared milestone guidance plus codex-specific guard strings.

/// Milestone instruction appended to all agent prompts
pub fn milestone_instruction() -> &'static str {
    "\nAfter completing each major step, print on its own line: \
     [MILESTONE] <brief description>"
}

/// Inject milestone guidance into a raw prompt
pub fn inject_milestone_prompt(raw: &str) -> String {
    let mut prompt = raw.to_string();
    prompt.push_str(milestone_instruction());
    prompt
}

/// No-op guard appended to codex prompts
pub fn codex_guard() -> &'static str {
    "\nIMPORTANT: If no changes are needed, do NOT create an empty commit. \
     Instead, print 'NO_CHANGES_NEEDED: <reason>' and exit."
}

/// Commit message instruction appended to codex prompts
pub fn codex_commit_msg(msg: &str) -> String {
    format!("\nCommit with message: '{msg}'")
}


/// Inject all codex templates into a raw prompt
pub fn inject_codex_prompt(raw: &str, commit_msg: Option<&str>) -> String {
    let mut prompt = raw.to_string();
    prompt.push_str(codex_guard());
    if let Some(msg) = commit_msg {
        prompt.push_str(&codex_commit_msg(msg));
    }
    prompt
}
