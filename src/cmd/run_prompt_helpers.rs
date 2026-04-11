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

/// Minimum prompt length to inject full methodology + gotchas.
/// Short prompts (trivial tasks) get references-only to avoid context pollution.
const SKILL_FULL_INJECT_MIN_CHARS: usize = 200;

pub(crate) fn inject_skill(prompt: &str, agent_kind: &AgentKind, requested_skills: &[String], raw_prompt_len: usize) -> Result<String> {
    if requested_skills.is_empty() { return Ok(prompt.to_string()); }
    let full_inject = raw_prompt_len >= SKILL_FULL_INJECT_MIN_CHARS;
    if !full_inject {
        aid_info!("[aid] Skill methodology skipped (short prompt, references only)");
    }
    let mut sections = Vec::new();
    for name in requested_skills {
        if full_inject {
            let skill_text = skills::load_skill(name)?;
            if let Some(gotchas) = skills::load_skill_gotchas(name, agent_kind) {
                sections.push(format!("--- Gotchas ---\n{gotchas}"));
            }
            sections.push(format!("--- Methodology ---\n{skill_text}"));
        }
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
    if *agent_kind == AgentKind::OpenCode || *agent_kind == AgentKind::Kilo {
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

pub(crate) fn expand_context_paths(specs: &[crate::context::ContextSpec]) -> Vec<String> { specs.iter().map(|spec| spec.file.clone()).collect() }

pub(crate) fn read_context_file(path: &str) -> Result<String> { std::fs::read_to_string(path).with_context(|| format!("Failed to read context file: {}", path)) }

pub(crate) fn format_context_block(path: &str, content: &str) -> String { format!("### {}\n```rust\n{}\n```", path, content.trim()) }

pub(crate) fn effective_skills(agent_kind: &AgentKind, args: &RunArgs) -> Vec<String> {
    let manual_skills: Vec<String> = args.skills.iter().filter(|skill| skill.as_str() != NO_SKILL_SENTINEL).cloned().collect();
    if !manual_skills.is_empty() || args.skills.iter().any(|skill| skill.as_str() == NO_SKILL_SENTINEL) { return manual_skills; }
    skills::auto_skills(agent_kind, args.worktree.is_some())
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
        let repo_dir = repo_path.map(|path| path.to_string()).unwrap_or(resolve_repo_path(args.dir.as_deref().unwrap_or("."))?);
        // Use explicit base_branch, or default to current branch (not just HEAD)
        // so worktrees inherit the latest state of whatever branch the user is on
        let base = args.base_branch.clone().or_else(|| current_branch(std::path::Path::new(&repo_dir)));
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
