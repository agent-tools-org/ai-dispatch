// Workgroup prompt composition for shared caller-injected context.
// Exports compose_prompt() so runs can reuse one group context across tasks.
// Depends on crate::types::Workgroup for the shared-context payload.

use crate::types::{Finding, Workgroup};

pub struct ComposedPrompt {
    pub text: String,
    pub omissions: Vec<String>,
}

pub fn compose_prompt(
    prompt: &str,
    file_context: Option<&str>,
    workgroup: Option<&Workgroup>,
    milestones: &[(String, String)],
    findings: &[Finding],
) -> ComposedPrompt {
    let mut sections = Vec::new();
    let mut omissions = Vec::new();

    if let Some(group) = workgroup.filter(|group| !group.shared_context.trim().is_empty()) {
        let shared_context = group.shared_context.trim();
        let context = format!("[Shared Context: {}]\n{shared_context}", group.name);
        sections.push(bound_auxiliary(&context, "Shared Context", &mut omissions));
    }
    if let Some(context) = file_context.filter(|context| !context.trim().is_empty()) {
        let context = format!("[Context]\n{}", context.trim());
        sections.push(bound_auxiliary(&context, "Context", &mut omissions));
    }
    append_entries(
        findings.iter().map(|f| {
            (
                f.source_task_id.as_deref().unwrap_or("manual"),
                f.content.as_str(),
            )
        }),
        "Shared Findings",
        &mut sections,
        &mut omissions,
    );
    append_entries(
        milestones
            .iter()
            .map(|(id, detail)| (id.as_str(), detail.as_str())),
        "Peer Milestones",
        &mut sections,
        &mut omissions,
    );

    let text = if sections.is_empty() {
        prompt.to_string()
    } else {
        sections.push(format!("[Task]\n{prompt}"));
        sections.join("\n\n")
    };
    ComposedPrompt { text, omissions }
}

const MAX_CONTEXT_BYTES: usize = 6000;

#[cfg(test)]
#[path = "workgroup_prompt_tests.rs"]
mod prompt_preservation;

pub(crate) fn bound_auxiliary(context: &str, label: &str, omissions: &mut Vec<String>) -> String {
    if context.len() <= MAX_CONTEXT_BYTES {
        return context.to_string();
    }
    let prefix = context.floor_char_boundary(MAX_CONTEXT_BYTES / 3);
    let suffix = context.ceil_char_boundary(context.len() - MAX_CONTEXT_BYTES * 2 / 3);
    let dropped = context[prefix..suffix].chars().count();
    let note = format!("{label}: {dropped} characters omitted");
    omissions.push(note.clone());
    format!("{}\n[... {note} ...]\n{}", &context[..prefix], &context[suffix..])
}

fn append_entries<'a>(
    entries: impl Iterator<Item = (&'a str, &'a str)>,
    label: &str,
    sections: &mut Vec<String>,
    omissions: &mut Vec<String>,
) {
    let mut dropped = 0;
    let lines = entries
        .map(|(source, content)| {
            let content = if content.len() > 500 {
                let end = content.floor_char_boundary(497);
                let count = content[end..].chars().count();
                dropped += count;
                format!("{}... [{count} characters of entry omitted]", &content[..end])
            } else {
                content.to_string()
            };
            format!("- [{source}] {content}")
        })
        .collect::<Vec<_>>()
        .join("\n");
    if dropped > 0 {
        omissions.push(format!("{label}: {dropped} characters of entries omitted"));
    }
    if !lines.is_empty() {
        sections.push(bound_auxiliary(&format!("[{label}]\n{lines}"), label, omissions));
    }
}

#[cfg(test)]
#[path = "workgroup_tests.rs"]
mod tests;
