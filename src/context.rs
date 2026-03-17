// Context injection: read files and extract specific items to prepend to prompts.
// Supports whole-file injection or targeted extraction of pub struct/trait/fn/enum.

use anyhow::{Context, Result};
use crate::skills;

#[derive(Debug, Clone)]
pub struct ContextSpec {
    pub file: String,
    pub items: Option<Vec<String>>,
}

/// Parse context specs from CLI args like `["src/types.rs:AgentKind,TaskId", "src/lib.rs"]`.
pub fn parse_context_specs(specs: &[String]) -> Result<Vec<ContextSpec>> {
    specs
        .iter()
        .map(|s| {
            let (raw_file, items) = if let Some((file, items_str)) = s.split_once(':') {
                let items: Vec<String> =
                    items_str.split(',').map(|i| i.trim().to_string()).collect();
                (file.to_string(), Some(items))
            } else {
                (s.to_string(), None)
            };
            Ok(ContextSpec {
                file: resolve_to_absolute(&raw_file),
                items,
            })
        })
        .collect()
}

fn resolve_to_absolute(path: &str) -> String {
    let path_buf = std::path::Path::new(path);
    if path_buf.is_absolute() {
        return path.to_string();
    }
    if let Ok(canonical) = path_buf.canonicalize() {
        return canonical.to_string_lossy().to_string();
    }
    if let Ok(cwd) = std::env::current_dir() {
        return cwd.join(path_buf).to_string_lossy().to_string();
    }
    path.to_string()
}

/// Read files and optionally extract matching pub items, formatted as markdown.
pub fn resolve_context(specs: &[ContextSpec]) -> Result<String> {
    let mut parts = Vec::new();
    let file_count = specs.len();

    for spec in specs {
        let content = std::fs::read_to_string(&spec.file)
            .with_context(|| format!("Failed to read context file: {}", spec.file))?;

        match &spec.items {
            None => {
                parts.push(format!(
                    "### {}\n```rust\n{}\n```",
                    spec.file,
                    content.trim()
                ));
            }
            Some(items) => {
                let extracted = extract_items(&content, items);
                if !extracted.is_empty() {
                    parts.push(format!(
                        "### {} ({})\n```rust\n{}\n```",
                        spec.file,
                        items.join(", "),
                        extracted.trim()
                    ));
                }
            }
        }
    }

    let context = parts.join("\n\n");
    let tokens = skills::estimate_tokens(&context);
    eprintln!("[aid] Context injected: {} files, ~{} tokens", file_count, tokens);
    Ok(context)
}

/// Prepend context block before the user's prompt.
pub fn inject_context(prompt: &str, context: &str) -> String {
    format!("[Context]\n{context}\n\n[Task]\n{prompt}")
}

/// Generate pointer-based context for agents that can read files themselves.
pub fn resolve_context_pointers(specs: &[ContextSpec]) -> String {
    let mut lines = vec!["[Context Files - read these before starting]".to_string()];
    for spec in specs {
        match &spec.items {
            None => lines.push(format!("- {}: read entire file", spec.file)),
            Some(items) => lines.push(format!("- {}: focus on {}", spec.file, items.join(", "))),
        }
    }
    lines.join("\n")
}

/// Extract blocks starting with `pub struct/trait/fn/enum <name>` until the next blank line
/// or next top-level pub item.
fn extract_items(content: &str, items: &[String]) -> String {
    let lines: Vec<&str> = content.lines().collect();
    let mut result = Vec::new();

    for item_name in items {
        let mut capturing = false;
        let mut brace_depth: i32 = 0;
        let mut block = Vec::new();

        for line in &lines {
            if !capturing {
                // Match `pub struct X`, `pub trait X`, `pub fn X`, `pub enum X`
                let trimmed = line.trim();
                let is_match = [
                    "pub struct ",
                    "pub trait ",
                    "pub fn ",
                    "pub enum ",
                    "pub type ",
                ]
                .iter()
                .any(|prefix| {
                    trimmed.starts_with(prefix)
                        && trimmed[prefix.len()..].starts_with(item_name.as_str())
                });
                if is_match {
                    capturing = true;
                    brace_depth = 0;
                    block.push(*line);
                    brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
                    brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
                    if brace_depth <= 0
                        && (line.contains(';') || (line.contains('{') && line.contains('}')))
                    {
                        capturing = false;
                        result.push(block.join("\n"));
                        block.clear();
                    }
                }
            } else {
                block.push(*line);
                brace_depth += line.chars().filter(|&c| c == '{').count() as i32;
                brace_depth -= line.chars().filter(|&c| c == '}').count() as i32;
                if brace_depth <= 0 {
                    capturing = false;
                    result.push(block.join("\n"));
                    block.clear();
                }
            }
        }
        if !block.is_empty() {
            result.push(block.join("\n"));
        }
    }

    result.join("\n\n")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use tempfile::tempdir_in;
    use tempfile::NamedTempFile;

    #[test]
    fn parse_specs_whole_file() {
        let specs = parse_context_specs(&["src/types.rs".to_string()]).unwrap();
        assert_eq!(specs.len(), 1);
        assert_eq!(specs[0].file, resolve_to_absolute("src/types.rs"));
        assert!(specs[0].items.is_none());
    }

    #[test]
    fn parse_specs_with_items() {
        let specs = parse_context_specs(&["src/types.rs:AgentKind,TaskId".to_string()]).unwrap();
        assert_eq!(specs[0].file, resolve_to_absolute("src/types.rs"));
        assert_eq!(specs[0].items.as_ref().unwrap(), &["AgentKind", "TaskId"]);
    }

    #[test]
    fn parse_specs_resolves_relative_paths_to_absolute() {
        let cwd = std::env::current_dir().unwrap();
        let dir = tempdir_in(&cwd).unwrap();
        let path = dir.path().join("context.rs");
        std::fs::write(&path, "pub struct RelativePath;\n").unwrap();
        let relative = path.strip_prefix(&cwd).unwrap().to_string_lossy().to_string();

        let specs = parse_context_specs(&[relative]).unwrap();

        assert_eq!(specs[0].file, path.canonicalize().unwrap().to_string_lossy());
    }

    #[test]
    fn parse_specs_keeps_absolute_paths_unchanged() {
        let file = NamedTempFile::new().unwrap();
        let absolute = file.path().to_string_lossy().to_string();

        let specs = parse_context_specs(std::slice::from_ref(&absolute)).unwrap();

        assert_eq!(specs[0].file, absolute);
    }

    #[test]
    fn resolve_whole_file() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(f, "pub struct Foo {{\n    x: i32,\n}}").unwrap();
        let specs = vec![ContextSpec {
            file: f.path().to_string_lossy().to_string(),
            items: None,
        }];
        let ctx = resolve_context(&specs).unwrap();
        assert!(ctx.contains("pub struct Foo"));
    }

    #[test]
    fn resolve_with_item_extraction() {
        let mut f = NamedTempFile::new().unwrap();
        writeln!(
            f,
            "pub struct Foo {{\n    x: i32,\n}}\n\npub struct Bar {{\n    y: i32,\n}}"
        )
        .unwrap();
        let specs = vec![ContextSpec {
            file: f.path().to_string_lossy().to_string(),
            items: Some(vec!["Foo".to_string()]),
        }];
        let ctx = resolve_context(&specs).unwrap();
        assert!(ctx.contains("pub struct Foo"));
        assert!(!ctx.contains("pub struct Bar"));
    }

    #[test]
    fn inject_context_format() {
        let result = inject_context("do something", "file contents here");
        assert!(result.starts_with("[Context]"));
        assert!(result.contains("[Task]"));
        assert!(result.contains("do something"));
    }

    #[test]
    fn resolve_context_pointers_whole_file() {
        let specs = vec![ContextSpec {
            file: "src/types.rs".to_string(),
            items: None,
        }];
        let result = resolve_context_pointers(&specs);
        assert!(result.starts_with("[Context Files - read these before starting]"));
        assert!(result.contains("- src/types.rs: read entire file"));
    }

    #[test]
    fn resolve_context_pointers_with_items() {
        let specs = vec![ContextSpec {
            file: "src/types.rs".to_string(),
            items: Some(vec!["AgentKind".to_string(), "TaskId".to_string()]),
        }];
        let result = resolve_context_pointers(&specs);
        assert!(result.contains("- src/types.rs: focus on AgentKind, TaskId"));
    }

    #[test]
    fn resolve_context_pointers_multiple_files() {
        let specs = vec![
            ContextSpec {
                file: "src/lib.rs".to_string(),
                items: None,
            },
            ContextSpec {
                file: "src/types.rs".to_string(),
                items: Some(vec!["Task".to_string()]),
            },
        ];
        let result = resolve_context_pointers(&specs);
        assert!(result.contains("- src/lib.rs: read entire file"));
        assert!(result.contains("- src/types.rs: focus on Task"));
    }
}
