// Agent TOML template builders for `aid agent add` and `aid agent fork`.
// Exports: custom_agent_template, build_builtin_agent_toml, title_case.
// Deps: agent selection capabilities, custom CapabilityScores, AgentKind.

use crate::agent::custom::CapabilityScores;
use crate::agent::selection::AGENT_CAPABILITIES;
use crate::agent::classifier::TaskCategory;
use crate::types::AgentKind;

const AGENT_TEMPLATE: &str = r#"# Custom agent definition for aid.
#
# Requirements: the command must be a non-interactive CLI that:
#   1. Accepts a prompt (via arg, flag, or stdin)
#   2. Performs the task autonomously
#   3. Exits when done
#
# Compatible CLIs: gemini, codex, opencode, cursor, claude, kilo, codebuff, aider, etc.
# NOT compatible: interactive/session-based tools without a stable non-interactive mode.

[agent]
id = "{name}"
display_name = "{display_name}"
command = "{name}"

# How to pass the prompt
prompt_mode = "arg"     # "arg", "stdin", or "flag"
# prompt_flag = "--message"  # uncomment if prompt_mode = "flag"

# CLI flag mappings (leave empty if not supported)
dir_flag = ""
model_flag = "--model"
output_flag = ""

# Fixed args always passed to the CLI
fixed_args = []

# Output parsing
streaming = false
output_format = "text"  # "text" or "jsonl"
# Strength categories for simple boosts (match TaskCategory strings, e.g. "research")
strengths = []

# Egress is derived from base_url (loopback => local). trust_tier is legacy and ignored.
# base_url = "http://127.0.0.1:11434/v1"

# Capability scores for auto-selection (0-10)
[agent.capabilities]
research = 3
simple_edit = 5
complex_impl = 5
frontend = 3
debugging = 5
testing = 4
refactoring = 5
documentation = 3
"#;

pub(super) fn custom_agent_template(name: &str, display_name: &str) -> String {
    AGENT_TEMPLATE
        .replace("{name}", name)
        .replace("{display_name}", display_name)
}

pub(super) fn build_builtin_agent_toml(target_name: &str, kind: AgentKind) -> String {
    let display_name = title_case(target_name);
    let Some((command, _, _, _, streaming)) = kind.profile() else {
        return String::new();
    };
    let caps = capability_scores_for(kind);
    let mut toml = String::new();
    toml.push_str(&format!(
        "# Forked from the built-in `{}` agent. Edit the entries below to customize this clone.\n",
        kind.as_str()
    ));
    toml.push_str("[agent]\n");
    toml.push_str(&format!("id = \"{target_name}\"\n"));
    toml.push_str(&format!("display_name = \"{display_name}\"\n"));
    toml.push_str(&format!(
        "command = \"{}\"  # CLI binary invoked by this agent\n",
        command
    ));
    toml.push_str("\n# How prompts reach the CLI\n");
    toml.push_str("prompt_mode = \"arg\"  # options: arg | flag | stdin\n");
    toml.push_str("# prompt_flag = \"--message\"  # enable when prompt_mode = \"flag\"\n\n");
    toml.push_str("# Optional CLI flags for directory, model, and output\n");
    toml.push_str("dir_flag = \"\"  # e.g. --dir or --workspace\n");
    toml.push_str("model_flag = \"\"  # e.g. --model\n");
    toml.push_str("output_flag = \"\"  # e.g. --output\n\n");
    toml.push_str("# Arguments that always run with this agent\n");
    toml.push_str("fixed_args = []\n\n");
    toml.push_str("# Streaming controls whether aid expects live JSONL events\n");
    toml.push_str(&format!("streaming = {}\n", streaming));
    toml.push_str("output_format = \"text\"  # text | jsonl\n\n");
    toml.push_str(
        "# Egress is derived from base_url (loopback => local for --egress local).\n",
    );
    toml.push_str("# base_url = \"http://127.0.0.1:11434/v1\"\n\n");
    toml.push_str("# Strength categories for auto-selection boosts\n");
    toml.push_str("strengths = []\n\n");
    toml.push_str("# Capability scores (0-10) guide auto-selection\n");
    toml.push_str("[agent.capabilities]\n");
    toml.push_str(&format!("research = {}\n", caps.research));
    toml.push_str(&format!("simple_edit = {}\n", caps.simple_edit));
    toml.push_str(&format!("complex_impl = {}\n", caps.complex_impl));
    toml.push_str(&format!("frontend = {}\n", caps.frontend));
    toml.push_str(&format!("debugging = {}\n", caps.debugging));
    toml.push_str(&format!("testing = {}\n", caps.testing));
    toml.push_str(&format!("refactoring = {}\n", caps.refactoring));
    toml.push_str(&format!("documentation = {}\n", caps.documentation));
    toml.push('\n');
    toml
}

fn capability_scores_for(kind: AgentKind) -> CapabilityScores {
    let mut scores = CapabilityScores::default();
    if let Some((_, entries)) = AGENT_CAPABILITIES.iter().find(|(k, _)| *k == kind) {
        for &(category, value) in *entries {
            match category {
                TaskCategory::Research => scores.research = value,
                TaskCategory::SimpleEdit => scores.simple_edit = value,
                TaskCategory::ComplexImpl => scores.complex_impl = value,
                TaskCategory::Frontend => scores.frontend = value,
                TaskCategory::Debugging => scores.debugging = value,
                TaskCategory::Testing => scores.testing = value,
                TaskCategory::Refactoring => scores.refactoring = value,
                TaskCategory::Documentation => scores.documentation = value,
            }
        }
    }
    scores
}

pub(super) fn title_case(name: &str) -> String {
    let pieces: Vec<String> = name
        .split(|c: char| c == '-' || c == '_' || c.is_whitespace())
        .filter(|seg| !seg.is_empty())
        .map(|seg| {
            let mut chars = seg.chars();
            let first = chars.next();
            match first {
                Some(f) => f.to_ascii_uppercase().to_string() + chars.as_str(),
                None => String::new(),
            }
        })
        .collect();
    if pieces.is_empty() {
        return name.to_string();
    }
    pieces.join(" ")
}
