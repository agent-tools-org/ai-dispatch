// Workgroup prompt composition for shared caller-injected context.
// Exports compose_prompt() so runs can reuse one group context across tasks.
// Depends on crate::types::Workgroup for the shared-context payload.

use crate::types::{Finding, Workgroup};

pub fn compose_prompt(
    prompt: &str,
    file_context: Option<&str>,
    workgroup: Option<&Workgroup>,
    milestones: &[(String, String)],
    findings: &[Finding],
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
    if !findings.is_empty() {
        let finding_lines = findings
            .iter()
            .map(|f| {
                let source = f.source_task_id.as_deref().unwrap_or("manual");
                let content = if f.content.len() > 500 {
                    let safe = f.content.floor_char_boundary(497);
                    format!("{}...", &f.content[..safe])
                } else {
                    f.content.clone()
                };
                format!("- [{source}] {content}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("[Shared Findings]\n{finding_lines}"));
    }
    if !milestones.is_empty() {
        let findings = milestones
            .iter()
            .map(|(task_id, detail)| {
                let detail = if detail.len() > 500 {
                    let safe = detail.floor_char_boundary(497);
                    format!("{}...", &detail[..safe])
                } else {
                    detail.clone()
                };
                format!("- [{task_id}] {detail}")
            })
            .collect::<Vec<_>>()
            .join("\n");
        sections.push(format!("[Peer Milestones]\n{findings}"));
    }
    if sections.is_empty() {
        return prompt.to_string();
    }

    sections.push(format!("[Task]\n{prompt}"));
    let full = sections.join("\n\n");
    compress_workgroup_context(&full)
}

const MAX_CONTEXT_CHARS: usize = 6000;

pub fn compress_workgroup_context(context: &str) -> String {
    if context.len() <= MAX_CONTEXT_CHARS {
        return context.to_string();
    }
    let lines: Vec<&str> = context.lines().collect();
    let header_count = lines.len().min(5);
    let header_budget = MAX_CONTEXT_CHARS / 3;
    let footer_budget = MAX_CONTEXT_CHARS * 2 / 3;

    let mut header = String::new();
    for line in lines.iter().take(header_count) {
        if header.len() + line.len() + 1 > header_budget {
            break;
        }
        header.push_str(line);
        header.push('\n');
    }

    let mut footer_lines: Vec<&str> = Vec::new();
    let mut footer_len = 0;
    for line in lines.iter().rev() {
        if footer_len + line.len() + 1 > footer_budget {
            break;
        }
        footer_len += line.len() + 1;
        footer_lines.push(line);
    }
    footer_lines.reverse();
    let footer = footer_lines.join("\n");
    let compressed = lines.len() - header_count - footer_lines.len();

    format!("{header}\n[... {compressed} lines compressed ...]\n\n{footer}")
}

#[cfg(test)]
mod tests {
    use super::compose_prompt;
    use crate::types::{Finding, Workgroup, WorkgroupId};
    use chrono::Local;

    fn make_group(shared_context: &str) -> Workgroup {
        Workgroup {
            id: WorkgroupId("wg-demo".to_string()),
            name: "dispatch".to_string(),
            shared_context: shared_context.to_string(),
            created_by: None,
            created_at: Local::now(),
            updated_at: Local::now(),
        }
    }

    #[test]
    fn returns_prompt_when_no_context_exists() {
        assert_eq!(compose_prompt("ship it", None, None, &[], &[]), "ship it");
    }

    #[test]
    fn includes_shared_and_file_context() {
        let prompt = compose_prompt(
            "ship it",
            Some("fn main() {}"),
            Some(&make_group("Rust workspace with shared target dir.")),
            &[],
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
        let prompt = compose_prompt("ship it", None, None, &milestones, &[]);

        assert!(prompt.contains("[Peer Milestones]"));
        assert!(prompt.contains("- [t-1000] finding one"));
        assert!(prompt.contains("- [t-1001] finding two"));
        assert!(prompt.contains("[Task]"));
    }

    #[test]
    fn short_context_unchanged() {
        let short = "hello world\nline two";
        assert_eq!(super::compress_workgroup_context(short), short);
    }

    #[test]
    fn long_context_compressed() {
        let lines: Vec<String> = (0..500).map(|i| format!("line {i}: some content here")).collect();
        let long = lines.join("\n");
        let compressed = super::compress_workgroup_context(&long);
        assert!(compressed.len() <= super::MAX_CONTEXT_CHARS + 200);
        assert!(compressed.contains("[... "));
        assert!(compressed.contains("lines compressed"));
    }

    #[test]
    fn compression_preserves_recent() {
        let mut lines: Vec<String> = (0..500).map(|i| format!("line {i}")).collect();
        lines.push("IMPORTANT_LAST_LINE".to_string());
        let long = lines.join("\n");
        let compressed = super::compress_workgroup_context(&long);
        assert!(compressed.contains("IMPORTANT_LAST_LINE"));
    }

    #[test]
    fn includes_findings_from_investigation() {
        let findings = vec![Finding {
            id: 1,
            workgroup_id: "wg-1".to_string(),
            content: "gamma can be zero".to_string(),
            source_task_id: Some("t-100".to_string()),
            severity: None,
            title: None,
            file: None,
            lines: None,
            category: None,
            confidence: None,
            created_at: Local::now(),
        }];
        let prompt = compose_prompt("investigate", None, None, &[], &findings);
        assert!(prompt.contains("[Shared Findings]"));
        assert!(prompt.contains("gamma can be zero"));
    }

    #[test]
    fn finding_content_truncated_at_500() {
        let long = "a".repeat(1000);
        let findings = vec![Finding {
            id: 1,
            workgroup_id: "wg-1".to_string(),
            content: long,
            source_task_id: Some("t-100".to_string()),
            severity: None,
            title: None,
            file: None,
            lines: None,
            category: None,
            confidence: None,
            created_at: Local::now(),
        }];

        let prompt = compose_prompt("investigate", None, None, &[], &findings);

        assert!(prompt.contains(&format!("- [t-100] {}...", "a".repeat(497))));
        assert!(!prompt.contains(&"a".repeat(498)));
    }

    #[test]
    fn milestone_content_truncated_at_500() {
        let milestones = vec![("t-1000".to_string(), "b".repeat(1000))];

        let prompt = compose_prompt("ship it", None, None, &milestones, &[]);

        assert!(prompt.contains(&format!("- [t-1000] {}...", "b".repeat(497))));
        assert!(!prompt.contains(&"b".repeat(498)));
    }

    #[test]
    fn short_finding_unchanged() {
        let findings = vec![Finding {
            id: 1,
            workgroup_id: "wg-1".to_string(),
            content: "short finding".to_string(),
            source_task_id: Some("t-100".to_string()),
            severity: None,
            title: None,
            file: None,
            lines: None,
            category: None,
            confidence: None,
            created_at: Local::now(),
        }];

        let prompt = compose_prompt("investigate", None, None, &[], &findings);

        assert!(prompt.contains("- [t-100] short finding"));
    }
}
