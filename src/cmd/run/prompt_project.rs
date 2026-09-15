// Project rule, state, and knowledge injection from the dispatch snapshot.
// Exports: inject_project_context for prompt assembly.
// Deps: ProjectConfig, prompt_context, prompt sanitization.
use super::{project, prompt_context, sanitize_injected_text};
use std::collections::HashSet;

pub(super) fn inject_project_context(mut effective_prompt: String, prompt: &str, project: Option<&project::ProjectConfig>, project_root: Option<&std::path::Path>) -> (String, HashSet<String>) {
    let mut project_topics: HashSet<String> = HashSet::new();

    // Inject project rules + knowledge if a project was detected
    if let Some(pc) = project {
        let rules_count = pc.rules.len();
        if !pc.rules.is_empty() {
            let rules_block = pc.rules.iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n");
            effective_prompt = format!("<aid-project-rules>\n{rules_block}\n</aid-project-rules>\n\n{effective_prompt}");
        }
        if let Some(state_block) = project_root.and_then(|root| prompt_context::inject_project_state(pc, root)) {
            let state_block = sanitize_injected_text(&state_block);
            effective_prompt = format!("{state_block}\n\n{effective_prompt}");
            aid_info!("[aid] Injected project state");
        }
        let knowledge_entries = project_root
            .map(project::read_project_knowledge)
            .unwrap_or_default();
        let total_knowledge = knowledge_entries.len();
        if total_knowledge > 0 {
            let relevant = prompt_context::select_relevant_entries(&knowledge_entries, prompt);
            if !relevant.is_empty() {
                for entry in &relevant {
                    project_topics.extend(prompt_context::extract_words(&entry.topic));
                }
                let knowledge_block = sanitize_injected_text(&prompt_context::format_knowledge_block(&pc.id, &relevant));
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
            aid_info!("[aid] Project '{}' detected: {} rule(s), {}/{} knowledge entries", pc.id, rules_count, relevant.len(), total_knowledge);
        } else if rules_count > 0 {
            aid_info!("[aid] Project '{}' detected: {} rule(s)", pc.id, rules_count);
        }
    }

    (effective_prompt, project_topics)
}
