// Workgroup prompt composition for shared caller-injected context.
// Exports compose_prompt() so runs can reuse one group context across tasks.
// Depends on crate::types::Workgroup for the shared-context payload.

use crate::types::Workgroup;

pub fn compose_prompt(
    prompt: &str,
    file_context: Option<&str>,
    workgroup: Option<&Workgroup>,
    milestones: &[(String, String)],
) -> String {
    let mut sections = Vec::new();

    if let Some(group) = workgroup.filter(|group| !group.shared_context.trim().is_empty()) {
        sections.push(format!(
            "[Shared Context: {}]\n{}",
            group.name,
            group.shared_context.trim()
        ));
    }
    if let Some(context) = file_context.filter(|context| !context.trim().is_empty()) {
        sections.push(format!("[Context]\n{}", context.trim()));
    }
    if !milestones.is_empty() {
        let findings = milestones
            .iter()
            .map(|(task_id, detail)| format!("- [{task_id}] {detail}"))
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("--- Shared Findings ---\n{findings}"));
    }
    if sections.is_empty() {
        return prompt.to_string();
    }

    sections.push(format!("[Task]\n{prompt}"));
    sections.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::compose_prompt;
    use crate::types::{Workgroup, WorkgroupId};
    use chrono::Local;

    fn make_group(shared_context: &str) -> Workgroup {
        Workgroup {
            id: WorkgroupId("wg-demo".to_string()),
            name: "dispatch".to_string(),
            shared_context: shared_context.to_string(),
            created_at: Local::now(),
            updated_at: Local::now(),
        }
    }

    #[test]
    fn returns_prompt_when_no_context_exists() {
        assert_eq!(compose_prompt("ship it", None, None, &[]), "ship it");
    }

    #[test]
    fn includes_shared_and_file_context() {
        let prompt = compose_prompt(
            "ship it",
            Some("fn main() {}"),
            Some(&make_group("Rust workspace with shared target dir.")),
            &[],
        );
        assert!(prompt.contains("[Shared Context: dispatch]"));
        assert!(prompt.contains("[Context]"));
        assert!(prompt.contains("[Task]"));
        assert!(prompt.contains("ship it"));
    }

    #[test]
    fn includes_shared_findings() {
        let milestones = vec![
            ("t-1000".to_string(), "finding one".to_string()),
            ("t-1001".to_string(), "finding two".to_string()),
        ];
        let prompt = compose_prompt("ship it", None, None, &milestones);

        assert!(prompt.contains("--- Shared Findings ---"));
        assert!(prompt.contains("- [t-1000] finding one"));
        assert!(prompt.contains("- [t-1001] finding two"));
        assert!(prompt.contains("[Task]"));
    }
}
