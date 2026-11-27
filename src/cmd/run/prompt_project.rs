// Project rule, state, and knowledge injection from the dispatch snapshot.
// Exports: inject_project_context for prompt assembly.
// Deps: ProjectConfig, prompt_context, prompt sanitization.
use super::{project, prompt_context, sanitize_injected_text};
use std::collections::HashSet;

pub(super) fn inject_project_context(
    mut effective_prompt: String, prompt: &str, project: Option<&project::ProjectConfig>,
    project_root: Option<&std::path::Path>, omissions: &mut Vec<String>,
) -> (String, HashSet<String>) {
    let mut project_topics: HashSet<String> = HashSet::new();

    // Inject project rules + knowledge if a project was detected
    if let Some(pc) = project {
        let rules_count = pc.rules.len();
        let stage_hint = pc.stage.and_then(project::ProjectStage::work_hint);
        if !pc.rules.is_empty() || stage_hint.is_some() {
            let rules_block = pc.rules.iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n");
            let mut rules_block = crate::task::workgroup::bound_auxiliary(&rules_block, "Project Rules", omissions);
            // The hint is appended after the bound, so the rules are cut exactly as without a stage.
            if let Some(hint) = stage_hint {
                if !rules_block.is_empty() { rules_block.push('\n'); }
                rules_block.push_str(&format!("- {hint}"));
            }
            effective_prompt = format!("<aid-project-rules>\n{rules_block}\n</aid-project-rules>\n\n{effective_prompt}");
        }
        let knowledge_entries = project_root
            .map(project::read_project_knowledge)
            .unwrap_or_default();
        let total_knowledge = knowledge_entries.len();
        if total_knowledge > 0 {
            let (relevant, omitted) = prompt_context::select_relevant_entries(&knowledge_entries, prompt);
            if !relevant.is_empty() || omitted > 0 {
                for entry in &relevant {
                    project_topics.extend(prompt_context::extract_words(&entry.topic));
                }
                let knowledge_block = sanitize_injected_text(
                    &prompt_context::format_knowledge_block(&pc.id, &relevant, omitted, omissions),
                );
                let knowledge_block = crate::task::workgroup::bound_auxiliary(
                    &knowledge_block, "Project Knowledge", omissions,
                );
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
            aid_info!("[aid] Project '{}' detected: {} rule(s), {}/{} knowledge entries", pc.id, rules_count, relevant.len(), total_knowledge);
        } else if rules_count > 0 {
            aid_info!("[aid] Project '{}' detected: {} rule(s)", pc.id, rules_count);
        }
    }

    (effective_prompt, project_topics)
}
