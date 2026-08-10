// Prompt resolution, context flags, skills, worktree paths, compaction.
use anyhow::{Context, Result};

use crate::{
    agent, compaction, skills, store::Store, templates, types::*,
};
use crate::cmd::run::{RunArgs, NO_SKILL_SENTINEL};

use super::run_process::current_branch;

pub(crate) fn resolve_prompt(prompt: &str, template: Option<&str>) -> Result<String> {
    let raw = prompt.to_string();
    if let Some(template) = template {
        let template_content = templates::load_template(template)?;
        Ok(templates::apply_template(&template_content, &raw))
    } else { Ok(raw) }
}

const RUST_CACHE_PROMPT_LINE: &str =
    "Rust project: CARGO_TARGET_DIR points at a warm shared target; do not override. Use 'aid build' for cargo check/clippy (clean, deduplicated compiler errors). Use 'aid test' for tests — zero-match filters fail and digests name which tests ran.";
const BATCH_SIBLING_LIMIT: usize = 10;
const BATCH_SIBLING_PROMPT_LIMIT: usize = 80;

pub(crate) fn sanitize_injected_text(text: &str) -> String {
    let mut result = Vec::new();
    let mut inside = false;
    for line in text.lines() {
        let trimmed = line.trim_start();
        if trimmed.starts_with("<aid-") && !trimmed.starts_with("</aid-") {
            inside = true;
            continue;
        }
        if trimmed.starts_with("</aid-") {
            inside = false;
            continue;
        }
        if !inside {
            result.push(line);
        }
    }
    result.join("\n")
}

fn truncate_batch_sibling_prompt(prompt: &str) -> String {
    let mut preview: String = prompt.chars().take(BATCH_SIBLING_PROMPT_LIMIT).collect();
    if prompt.chars().count() > BATCH_SIBLING_PROMPT_LIMIT {
        preview.push_str("...");
    }
    preview
}

pub(super) fn format_batch_siblings(siblings: &[(String, String, String)]) -> String {
    let shown = siblings
        .iter()
        .take(BATCH_SIBLING_LIMIT)
        .map(|(name, agent, prompt)| {
            format!(
                "- \"{}\" ({}): {}",
                name,
                agent,
                truncate_batch_sibling_prompt(prompt)
            )
        })
        .collect::<Vec<_>>();
    let remaining = siblings.len().saturating_sub(BATCH_SIBLING_LIMIT);
    let mut lines = vec![
        "<aid-batch-siblings>".to_string(),
        "Other tasks running in this batch:".to_string(),
    ];
    lines.extend(shown);
    if remaining > 0 {
        lines.push(format!("+ {remaining} more"));
    }
    lines.push("</aid-batch-siblings>".to_string());
    lines.join("\n")
}

pub(crate) fn inject_skill(
    prompt: &str,
    agent_kind: &AgentKind,
    requested_skills: &[String],
    required: bool,
) -> Result<String> {
    if requested_skills.is_empty() { return Ok(prompt.to_string()); }
    let mut sections = Vec::new();
    for name in requested_skills {
        let skill_text = match skills::load_skill(name) {
            Ok(text) => text,
            Err(error) if !required => {
                aid_warn!("[aid] Auto-applied skill unavailable: {name} ({error})");
                continue;
            }
            Err(error) => return Err(error),
        };
        if let Some(gotchas) = skills::load_skill_gotchas(name, agent_kind) {
            sections.push(format!("--- Gotchas ---\n{gotchas}"));
        }
        sections.push(format!("--- Methodology ---\n{skill_text}"));
        let scripts = skills::load_skill_scripts(name);
        if !scripts.is_empty() {
            sections.push(
                format!(
                    "{}\n{}",
                    skills::format_script_instructions(&scripts)
                        .replacen("--- Available Tools ---", "--- Available Scripts ---", 1),
                    scripts
                        .iter()
                        .map(|script| format!("- {}", script.path.display()))
                        .collect::<Vec<_>>()
                        .join("\n")
                ),
            );
        }
        let references = skills::list_skill_references(name);
        if !references.is_empty() {
            sections.push(format!(
                "--- References (read on demand) ---\nFor detailed reference, read these files when needed:\n{}",
                references
                    .iter()
                    .map(|path| format!("- {path}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            ));
        }
    }
    Ok(format!("{prompt}\n\n{}", sections.join("\n\n")))
}

pub(crate) fn build_context_flags(agent_kind: &AgentKind, context_args: &[String]) -> Result<(Option<String>, Vec<String>)> {
    if context_args.is_empty() { return Ok((None, vec![])); }
    let specs = crate::context::parse_context_specs(context_args)?;
    let context_files = expand_context_paths(&specs);
    if *agent_kind == AgentKind::OpenCode || *agent_kind == AgentKind::Kilo || *agent_kind == AgentKind::MiMoCode {
        let hints: Vec<String> = specs.iter().filter_map(|spec| spec.items.as_ref().map(|items| format!("Focus on: {} in {}", items.join(", "), spec.file))).collect();
        let file_context = (!hints.is_empty()).then(|| hints.join("\n"));
        return Ok((file_context, context_files));
    }
    if agent::agent_has_fs_access(agent_kind) { return Ok((Some(crate::context::resolve_context_pointers(&specs)), vec![])); }
    let file_context = if specs.iter().all(|spec| spec.items.is_none()) {
        let mut blocks = Vec::new();
        for spec in &specs { let content = read_context_file(&spec.file)?; blocks.push(format_context_block(&spec.file, &content)); }
        blocks.join("\n\n")
    } else { crate::context::resolve_context(&specs)? };
    Ok((Some(file_context), vec![]))
}

pub(crate) fn rust_cache_prompt_line(dir: Option<&str>) -> Option<&'static str> {
    agent::is_rust_project(dir).then_some(RUST_CACHE_PROMPT_LINE)
}

pub(crate) fn expand_context_paths(specs: &[crate::context::ContextSpec]) -> Vec<String> { specs.iter().map(|spec| spec.file.clone()).collect() }

pub(crate) fn read_context_file(path: &str) -> Result<String> { std::fs::read_to_string(path).with_context(|| format!("Failed to read context file: {}", path)) }

pub(crate) fn format_context_block(path: &str, content: &str) -> String { format!("### {}\n```rust\n{}\n```", path, content.trim()) }

/// Skills come from the caller, then the project, then nowhere.
///
/// The third step used to be `skills::auto_skills`, which chose by **agent kind
/// alone** and never looked at the task: every implementation CLI was handed
/// `implementer`, and gemini and agy were handed `researcher`, whatever the
/// work actually was. A skill injects a substantial block of methodology text
/// and a persona, so that guess spent the caller's tokens steering the agent
/// toward something nobody had asked for.
///
/// A project that wants the old behaviour declares it once, in
/// `.aid/project.toml`: `skills = ["implementer"]`.
pub(crate) fn effective_skills(args: &RunArgs) -> Vec<String> {
    let project_skills = crate::project::detect_project()
        .map(|config| config.skills)
        .unwrap_or_default();
    effective_skills_with(args, project_skills)
}

/// The project default is a parameter so a caller — a test above all — can
/// state it rather than inherit whatever `.aid/project.toml` the developer
/// happens to have. The test for "omitting `--skill` invents nothing" passed
/// for months only because this repo's project file failed to deserialize and
/// `detect_project` discarded the error.
pub(crate) fn effective_skills_with(args: &RunArgs, project_skills: Vec<String>) -> Vec<String> {
    let declared: Vec<String> = args
        .skills
        .iter()
        .filter(|skill| skill.as_str() != NO_SKILL_SENTINEL)
        .cloned()
        .collect();
    if !declared.is_empty() {
        return declared;
    }
    // An explicit "no skills" is a decision, not an omission, so it must not
    // fall through to the project default.
    if args.skills.iter().any(|skill| skill.as_str() == NO_SKILL_SENTINEL) {
        return Vec::new();
    }
    project_skills
}

pub(crate) fn resolve_repo_path(path: &str) -> Result<String> {
    crate::repo_root::resolve_git_root_string(path)
}

pub(crate) fn resolve_dir_in_target(base_dir: &str, dir: Option<&str>, repo_dir: Option<&str>) -> String {
    let Some(dir) = dir else { return base_dir.to_string() };
    let dir_path = std::path::Path::new(dir);
    if dir_path == std::path::Path::new(".") { return base_dir.to_string(); }
    if dir_path.is_absolute() && let Some(repo_dir) = repo_dir && let Ok(relative_dir) = dir_path.strip_prefix(repo_dir) {
        return std::path::Path::new(base_dir).join(relative_dir).to_string_lossy().to_string();
    }
    if dir_path.is_absolute() { return dir.to_string(); }
    std::path::Path::new(base_dir).join(dir_path).to_string_lossy().to_string()
}

/// Returns (wt_path, wt_branch, effective_dir, resolved_repo_path, fresh_worktree).
/// The resolved_repo_path is always populated when a worktree is created, even if --repo wasn't passed.
type WorktreePaths = (Option<String>, Option<String>, Option<String>, Option<String>, bool);
pub(crate) fn resolve_worktree_paths(args: &RunArgs, repo_path: Option<&str>) -> Result<WorktreePaths> {
    if let Some(ref branch) = args.worktree {
        anyhow::ensure!(
            !args.read_only,
            "--read-only cannot be used with --worktree"
        );
        let repo_dir = match repo_path {
            Some(path) => path.to_string(),
            None => resolve_repo_path(args.dir.as_deref().unwrap_or("."))?,
        };
        // Use explicit base_branch, or default to current branch (not just HEAD)
        // so worktrees inherit the latest state of whatever branch the user is on
        let default_base = args.base_branch.clone().or_else(|| current_branch(std::path::Path::new(&repo_dir)));
        let base = if args.base_branch.is_none() {
            crate::worktree::branch_tip_resume_base(std::path::Path::new(&repo_dir), branch)?
                .or(default_base)
        } else {
            default_base
        };
        let info = crate::worktree::create_worktree(std::path::Path::new(&repo_dir), branch, base.as_deref())?;
        let p = info.path.to_string_lossy().to_string();
        return Ok((Some(p.clone()), Some(info.branch), Some(resolve_dir_in_target(&p, args.dir.as_deref(), Some(&repo_dir))), Some(repo_dir), info.created));
    }
    if let Some(repo_dir) = repo_path {
        return Ok((None, None, Some(resolve_dir_in_target(repo_dir, args.dir.as_deref(), Some(repo_dir))), Some(repo_dir.to_string()), false));
    }
    Ok((None, None, args.dir.clone(), None, false))
}

pub(crate) fn load_workgroup(store: &Store, group_id: Option<&str>) -> Result<Option<Workgroup>> {
    let Some(group_id) = group_id else { return Ok(None) };
    if let Some(wg) = store.get_workgroup(group_id)? {
        return Ok(Some(wg));
    }
    println!("[aid] Auto-created workgroup '{}'", group_id);
    Ok(Some(store.create_workgroup(group_id, "", Some("auto"), Some(group_id))?))
}

pub(crate) fn maybe_compact_prompt(prompt: &str, max_tokens: usize) -> String {
    let before = templates::estimate_tokens(prompt);
    if before <= max_tokens {
        return prompt.to_string();
    }
    let candidate = prompt
        .split("\n\n")
        .filter_map(|section| {
            let trimmed = section.trim_start();
            if trimmed.is_empty() || trimmed.starts_with("[Task]") {
                return None;
            }
            if trimmed.starts_with('[') || trimmed.starts_with("---") {
                Some((section, templates::estimate_tokens(section)))
            } else {
                None
            }
        })
        .max_by_key(|(_, tokens)| *tokens);
    let Some((section, section_tokens)) = candidate else {
        return prompt.to_string();
    };
    let excess = before.saturating_sub(max_tokens);
    let target_tokens = section_tokens.saturating_sub(excess);
    let compacted = compaction::compact_text(section, target_tokens);
    if compacted == section {
        return prompt.to_string();
    }
    let result = prompt.replacen(section, &compacted, 1);
    let after = templates::estimate_tokens(&result);
    aid_info!("[aid] Compacted prompt from ~{before} to ~{after} tokens");
    result
}
