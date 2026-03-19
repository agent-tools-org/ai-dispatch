// Prompt and run helpers for `aid run`.
// Exports: build_prompt_bundle(), resolve_prompt(), build_context_flags(), run_agent_process_impl().
// Deps: context, templates, workgroup, skills, watcher, store.
use anyhow::{Context, Result};
use serde_json;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::process::Command;
use crate::{
    agent, project, skills, store::Store, templates, team, types::*, watcher,
};
use crate::cmd::summary::{format_summary_for_injection, CompletionSummary};
use crate::store::TaskCompletionUpdate;
mod prompt_context;
#[path = "run_output.rs"]
mod run_output;
#[path = "run_verify.rs"]
mod run_verify;
#[path = "run_scope.rs"]
mod run_scope;
pub(super) use run_output::{fill_empty_output_from_log, clean_output_if_jsonl, output_file_instruction};
pub(super) use run_scope::warn_agent_committed_files_outside_scope;
pub(super) use run_verify::{maybe_auto_retry_after_verify_failure_impl, maybe_cleanup_fast_fail_impl, maybe_verify_impl};
use super::RunArgs;

const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";
const PROMPT_TOKEN_LIMIT: usize = 30_000;
const BATCH_SIBLING_LIMIT: usize = 10;
const BATCH_SIBLING_PROMPT_LIMIT: usize = 80;

pub(super) struct PromptBundle { pub effective_prompt: String, pub context_files: Vec<String>, pub prompt_tokens: i64, pub injected_memory_ids: Vec<String> }

fn sanitize_injected_text(text: &str) -> String {
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

pub(super) fn build_prompt_bundle(store: &Store, args: &RunArgs, agent_kind: &AgentKind, workgroup: Option<&Workgroup>, requested_skills: &[String], current_task_id: &str) -> Result<PromptBundle> {
    let (file_context, context_files) = build_context_flags(agent_kind, &args.context)?;
    let milestones = if let Some(group_id) = args.group.as_deref() {
        store.get_workgroup_milestones(group_id)?
    } else {
        vec![]
    };
    let findings = if let Some(group_id) = args.group.as_deref() {
        store.list_findings(group_id)?
    } else {
        vec![]
    };
    let prompt = resolve_prompt(&args.prompt, args.template.as_deref())?;
    let mut effective_prompt = crate::workgroup::compose_prompt(
        &prompt,
        file_context.as_deref(),
        workgroup,
        &milestones,
        &findings,
    );
    let (edit_guard, milestone_instr) = templates::shared_system_fragments(&prompt);
    if let Some(guard) = edit_guard { effective_prompt = format!("{guard}{effective_prompt}"); }
    effective_prompt.push_str(milestone_instr);

    if let Some(parent_id) = args.parent_task_id.as_deref()
        && let Some(parent) = store.get_task(parent_id)?
        && parent.status == TaskStatus::Done
        && let Some(summary_json) = store.get_completion_summary(parent_id)?
        && let Ok(summary) = serde_json::from_str::<CompletionSummary>(&summary_json)
    {
        let summary_block = format_summary_for_injection(&summary);
        effective_prompt = format!("{summary_block}\n\n{effective_prompt}");
    }
    if let Some(ref group_id) = args.group
        && !matches!(agent_kind, AgentKind::OpenCode | AgentKind::Kilo)
    {
        let sibling_summaries = prompt_context::collect_sibling_summaries(store, group_id, current_task_id)?;
        if !sibling_summaries.is_empty() {
            let block = sanitize_injected_text(&crate::cmd::summary::format_sibling_summaries(&sibling_summaries));
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }
    let mut effective_prompt = inject_skill(&effective_prompt, agent_kind, requested_skills)?;
    let mut injected_memory_ids = Vec::new();

    // Inject relevant memories from past tasks
    if let Some((memory_block, memory_ids)) = prompt_context::inject_memories(store, &args.prompt, 10)? {
        let memory_block = sanitize_injected_text(&memory_block);
        effective_prompt = format!("{memory_block}\n\n{effective_prompt}");
        injected_memory_ids = memory_ids;
    }

    let mut project_topics: HashSet<String> = HashSet::new();

    // Inject project rules + knowledge if a project was detected
    if let Some(pc) = project::detect_project() {
        let rules_count = pc.rules.len();
        if !pc.rules.is_empty() {
            let rules_block = pc.rules.iter()
                .map(|r| format!("- {r}"))
                .collect::<Vec<_>>()
                .join("\n");
            effective_prompt = format!("<aid-project-rules>\n{rules_block}\n</aid-project-rules>\n\n{effective_prompt}");
        }
        let knowledge_entries = prompt_context::detect_project_path()
            .map(|path| project::read_project_knowledge(std::path::Path::new(&path)))
            .unwrap_or_default();
        let total_knowledge = knowledge_entries.len();
        if total_knowledge > 0 {
            let relevant = prompt_context::select_relevant_entries(&knowledge_entries, &args.prompt);
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

    // Inject team rules + knowledge if --team was specified
    if let Some(ref team_id) = args.team {
        if let Some(tc) = team::resolve_team(team_id)
            && !tc.rules.is_empty() {
                let rules_block = tc.rules.iter()
                    .map(|r| format!("- {r}"))
                    .collect::<Vec<_>>()
                    .join("\n");
                effective_prompt = format!("<aid-team-rules>\n{rules_block}\n</aid-team-rules>\n\n{effective_prompt}");
                aid_info!("[aid] Injected {} team rule(s)", tc.rules.len());
            }
        let entries = team::read_knowledge_entries(team_id);
        let total_entries = entries.len();
        if total_entries > 0 {
            let relevant = prompt_context::select_relevant_entries(&entries, &args.prompt);
            let relevant: Vec<_> = if project_topics.is_empty() {
                relevant
            } else {
                relevant
                    .into_iter()
                    .filter(|entry| {
                        let entry_topic_words = prompt_context::extract_words(&entry.topic);
                        let overlap = entry_topic_words
                            .iter()
                            .filter(|word| project_topics.contains(*word))
                            .count();
                        let total = entry_topic_words.len().max(1);
                        (overlap as f64 / total as f64) < 0.5
                    })
                    .collect()
            };
            aid_info!("[aid] Injected {}/{} knowledge entries (relevance-filtered)", relevant.len(), total_entries);
            if !relevant.is_empty() {
                let knowledge_block = sanitize_injected_text(&prompt_context::format_knowledge_block(team_id, &relevant));
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
        }
    }

    // Inject output from previous tasks (--context-from)
    if !args.context_from.is_empty()
        && let Some(block) = prompt_context::resolve_context_from(store, &args.context_from)?
    {
        let token_count = templates::estimate_tokens(&block);
        aid_info!("[aid] Injected context from {} task(s) (~{token_count} tokens)", args.context_from.len());
        effective_prompt = format!("{block}\n\n{effective_prompt}");
    }

    if let Ok(shared_dir) = std::env::var("AID_SHARED_DIR") {
        effective_prompt = format!(
            "[Shared Directory]\nA shared directory is available at: {shared_dir}\nWrite files here that other tasks in the batch need to read.\nRead files here that other tasks may have produced.\n\n{effective_prompt}"
        );
    }

    // Inject workspace path if workgroup has one (appended to avoid commit message pollution)
    if let Some(ref group_id) = args.group {
        let workspace = crate::paths::workspace_dir(group_id)?;
        if workspace.is_dir() {
            effective_prompt = format!(
                "{effective_prompt}\n\n<aid-system-context>\n[Shared Workspace] Path: {} — use for intermediate artifacts and inter-agent communication.\n</aid-system-context>",
                workspace.display()
            );
        }
    }

    if !args.batch_siblings.is_empty() {
        effective_prompt = format!(
            "{effective_prompt}\n\n{}",
            format_batch_siblings(&args.batch_siblings)
        );
    }
    if args.output.is_some() {
        effective_prompt = format!("{effective_prompt}\n\n{}", output_file_instruction());
    }

    // Compact prompt if it exceeds token budget
    let effective_prompt = maybe_compact_prompt(&effective_prompt, PROMPT_TOKEN_LIMIT);
    let prompt_tokens = templates::estimate_tokens(&effective_prompt) as i64;
    Ok(PromptBundle { effective_prompt, context_files, prompt_tokens, injected_memory_ids })
}

pub(super) fn resolve_prompt(prompt: &str, template: Option<&str>) -> Result<String> {
    let raw = prompt.to_string();
    if let Some(template) = template {
        let template_content = templates::load_template(template)?;
        Ok(templates::apply_template(&template_content, &raw))
    } else { Ok(raw) }
}

pub(super) fn inject_skill(prompt: &str, agent_kind: &AgentKind, requested_skills: &[String]) -> Result<String> {
    if requested_skills.is_empty() { return Ok(prompt.to_string()); }
    let mut sections = Vec::new();
    for name in requested_skills {
        let skill_text = skills::load_skill(name)?;
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

pub(super) fn build_context_flags(agent_kind: &AgentKind, context_args: &[String]) -> Result<(Option<String>, Vec<String>)> {
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

pub(super) fn expand_context_paths(specs: &[crate::context::ContextSpec]) -> Vec<String> { specs.iter().map(|spec| spec.file.clone()).collect() }

pub(super) fn read_context_file(path: &str) -> Result<String> { std::fs::read_to_string(path).with_context(|| format!("Failed to read context file: {}", path)) }

pub(super) fn format_context_block(path: &str, content: &str) -> String { format!("### {}\n```rust\n{}\n```", path, content.trim()) }

pub(super) fn effective_skills(agent_kind: &AgentKind, args: &RunArgs) -> Vec<String> {
    let manual_skills: Vec<String> = args.skills.iter().filter(|skill| skill.as_str() != super::NO_SKILL_SENTINEL).cloned().collect();
    if !manual_skills.is_empty() || args.skills.iter().any(|skill| skill.as_str() == super::NO_SKILL_SENTINEL) { return manual_skills; }
    skills::auto_skills(agent_kind, args.worktree.is_some())
}

pub(super) fn resolve_repo_path(path: &str) -> Result<String> {
    let out = std::process::Command::new("git").args(["-C", path, "rev-parse", "--show-toplevel"]).output().context("Failed to run git")?;
    anyhow::ensure!(out.status.success(), "Not a git repository: {path}");
    Ok(String::from_utf8_lossy(&out.stdout).trim().to_string())
}

pub(super) fn resolve_dir_in_target(base_dir: &str, dir: Option<&str>, repo_dir: Option<&str>) -> String {
    let Some(dir) = dir else { return base_dir.to_string() };
    let dir_path = std::path::Path::new(dir);
    if dir_path == std::path::Path::new(".") { return base_dir.to_string(); }
    if dir_path.is_absolute() && let Some(repo_dir) = repo_dir && let Ok(relative_dir) = dir_path.strip_prefix(repo_dir) {
        return std::path::Path::new(base_dir).join(relative_dir).to_string_lossy().to_string();
    }
    if dir_path.is_absolute() { return dir.to_string(); }
    std::path::Path::new(base_dir).join(dir_path).to_string_lossy().to_string()
}

/// Returns (wt_path, wt_branch, effective_dir, resolved_repo_path).
/// The resolved_repo_path is always populated when a worktree is created, even if --repo wasn't passed.
type WorktreePaths = (Option<String>, Option<String>, Option<String>, Option<String>);
pub(super) fn resolve_worktree_paths(args: &RunArgs, repo_path: Option<&str>) -> Result<WorktreePaths> {
    if let Some(ref branch) = args.worktree {
        let repo_dir = repo_path.map(|path| path.to_string()).unwrap_or(resolve_repo_path(args.dir.as_deref().unwrap_or("."))?);
        // Use explicit base_branch, or default to current branch (not just HEAD)
        // so worktrees inherit the latest state of whatever branch the user is on
        let base = args.base_branch.clone().or_else(|| current_branch(std::path::Path::new(&repo_dir)));
        let info = crate::worktree::create_worktree(std::path::Path::new(&repo_dir), branch, base.as_deref())?;
        let p = info.path.to_string_lossy().to_string();
        return Ok((Some(p.clone()), Some(info.branch), Some(resolve_dir_in_target(&p, args.dir.as_deref(), Some(&repo_dir))), Some(repo_dir)));
    }
    if let Some(repo_dir) = repo_path {
        return Ok((None, None, Some(resolve_dir_in_target(repo_dir, args.dir.as_deref(), Some(repo_dir))), Some(repo_dir.to_string())));
    }
    Ok((None, None, args.dir.clone(), None))
}

pub(super) fn load_workgroup(store: &Store, group_id: Option<&str>) -> Result<Option<Workgroup>> {
    let Some(group_id) = group_id else { return Ok(None) };
    if let Some(wg) = store.get_workgroup(group_id)? {
        return Ok(Some(wg));
    }
    println!("[aid] Auto-created workgroup '{}'", group_id);
    Ok(Some(store.create_workgroup(group_id, "", Some("auto"), Some(group_id))?))
}

pub(super) struct RunProcessArgs<'a> {
    pub agent: &'a dyn crate::agent::Agent,
    pub cmd: Command,
    pub task_id: &'a TaskId,
    pub store: &'a Arc<Store>,
    pub log_path: &'a std::path::Path,
    pub output_path: Option<&'a str>,
    pub model: Option<&'a str>,
    pub streaming: bool,
    pub workgroup_id: Option<&'a str>,
}

/// Clean up any lingering child processes in the process group (Unix only).
#[cfg(unix)]
fn cleanup_process_group(child: &tokio::process::Child) {
    if let Some(pid) = child.id() {
        unsafe {
            libc::kill(-(pid as i32), libc::SIGTERM);
        }
    }
}

pub(super) async fn run_agent_process_impl(args: RunProcessArgs<'_>) -> Result<()> {
    let RunProcessArgs {
        agent,
        mut cmd,
        task_id,
        store,
        log_path,
        output_path,
        model,
        streaming,
        workgroup_id,
    } = args;
    let start = std::time::Instant::now();
    #[cfg(unix)]
    cmd.process_group(0);
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;
    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id, None).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out, workgroup_id).await?
    };
    // SIGTERM orphaned child processes — no sleep needed on normal exit
    #[cfg(unix)]
    cleanup_process_group(&child);
    let _ = child.kill().await;
    let _ = child.wait().await;
    let output_path = output_path.map(std::path::Path::new);
    fill_empty_output_from_log(log_path, output_path)?;
    if let Some(out_path) = output_path {
        clean_output_if_jsonl(out_path)?;
    }
    let duration_ms = start.elapsed().as_millis() as i64;
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| info.tokens.and_then(|tokens| crate::cost::estimate_cost(tokens, final_model, agent.kind())));
    store.update_task_completion(TaskCompletionUpdate {
        id: task_id.as_str(),
        status: info.status,
        tokens: info.tokens,
        duration_ms,
        model: final_model,
        cost_usd,
        exit_code: info.exit_code,
    })?;
    let duration_str = format_duration(duration_ms);
    let tokens_str = info.tokens.map(|t| format!(", {} tokens", t)).unwrap_or_default();
    let cost_str = if cost_usd.is_some() { format!(", {}", crate::cost::format_cost(cost_usd)) } else { String::new() };
    let fail_reason = if info.status == TaskStatus::Failed {
        store.latest_error(task_id.as_str())
            .map(|r| format!("\n[aid] Reason: {r}"))
            .unwrap_or_default()
    } else {
        String::new()
    };
    println!("Task {} {} ({}{}{}){}", task_id, info.status.label(), duration_str, tokens_str, cost_str, fail_reason);
    Ok(())
}

pub(super) fn notify_task_completion(store: &Store, task_id: &TaskId) -> Result<()> {
    if let Some(task) = store.get_task(task_id.as_str())? {
        crate::notify::notify_completion(&task);
    }
    Ok(())
}

/// Get the current branch name of a git repo (None if detached HEAD or error)
fn current_branch(repo_dir: &std::path::Path) -> Option<String> {
    let out = std::process::Command::new("git")
        .args(["-C", &repo_dir.to_string_lossy(), "rev-parse", "--abbrev-ref", "HEAD"])
        .output()
        .ok()?;
    if !out.status.success() { return None; }
    let branch = String::from_utf8(out.stdout).ok()?.trim().to_string();
    if branch == "HEAD" { return None; } // detached HEAD
    Some(branch)
}

pub(super) fn inherit_retry_base_branch_impl(repo_dir: Option<&str>, task: &Task, retry_args: &mut RunArgs) {
    if retry_args.base_branch.is_some() || retry_args.worktree.is_none() { return; }
    let Some(branch) = task.worktree_branch.as_deref() else { return };
    if retry_args.worktree.as_deref() == Some(branch) { return; }
    let repo_dir = std::path::Path::new(task.repo_path.as_deref().or(retry_args.repo.as_deref()).or(repo_dir).unwrap_or("."));
    if let Ok(true) = crate::worktree::branch_has_commits_ahead_of_main(repo_dir, branch) { retry_args.base_branch = Some(branch.to_string()); }
}

fn retry_target(task: &Task) -> (Option<String>, Option<String>) {
    match task.worktree_path.as_ref() {
        Some(path) if std::path::Path::new(path).exists() => (Some(path.clone()), None),
        Some(_) => (None, task.worktree_branch.clone()),
        None => (None, None),
    }
}

fn format_duration(ms: i64) -> String {
    let secs = ms / 1000;
    if secs < 60 { format!("{secs}s") } else { format!("{}m {:02}s", secs / 60, secs % 60) }
}

fn maybe_compact_prompt(prompt: &str, max_tokens: usize) -> String {
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
    let compacted = crate::compaction::compact_text(section, target_tokens);
    if compacted == section {
        return prompt.to_string();
    }
    let result = prompt.replacen(section, &compacted, 1);
    let after = templates::estimate_tokens(&result);
    aid_info!("[aid] Compacted prompt from ~{before} to ~{after} tokens");
    result
}

#[cfg(test)]
#[path = "run_prompt_tests.rs"]
mod tests;
