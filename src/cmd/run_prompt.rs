// Prompt and run helpers for `aid run`.
// Exports: build_prompt_bundle(), resolve_prompt(), build_context_flags(), run_agent_process_impl().
// Deps: context, templates, workgroup, skills, watcher, store.
use anyhow::{Context, Result};
use chrono::Local;
use serde_json;
use std::collections::HashSet;
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tokio::process::Command;
use crate::{
    agent, project, skills, store::Store, templates, team, types::*, watcher, worktree,
};
use crate::cmd::summary::{format_summary_for_injection, CompletionSummary};
use crate::store::TaskCompletionUpdate;
mod prompt_context;
use super::RunArgs;

const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";
const PROMPT_TOKEN_LIMIT: usize = 30_000;
const BATCH_SIBLING_LIMIT: usize = 10;
const BATCH_SIBLING_PROMPT_LIMIT: usize = 80;

pub(super) struct PromptBundle { pub effective_prompt: String, pub context_files: Vec<String>, pub prompt_tokens: i64, pub injected_memory_ids: Vec<String> }

fn sanitize_injected_text(text: &str) -> String {
    text.lines()
        .filter(|line| {
            let trimmed = line.trim_start();
            !trimmed.starts_with("<aid-") && !trimmed.starts_with("</aid-")
        })
        .collect::<Vec<_>>()
        .join("\n")
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
    if let Some(ref group_id) = args.group {
        let sibling_summaries = prompt_context::collect_sibling_summaries(store, group_id, current_task_id)?;
        if !sibling_summaries.is_empty() {
            let block = sanitize_injected_text(&crate::cmd::summary::format_sibling_summaries(&sibling_summaries));
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }
    let mut effective_prompt = inject_skill(&effective_prompt, requested_skills)?;
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
            eprintln!("[aid] Project '{}' detected: {} rule(s), {}/{} knowledge entries", pc.id, rules_count, relevant.len(), total_knowledge);
        } else if rules_count > 0 {
            eprintln!("[aid] Project '{}' detected: {} rule(s)", pc.id, rules_count);
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
                eprintln!("[aid] Injected {} team rule(s)", tc.rules.len());
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
            eprintln!("[aid] Injected {}/{} knowledge entries (relevance-filtered)", relevant.len(), total_entries);
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
        eprintln!("[aid] Injected context from {} task(s) (~{token_count} tokens)", args.context_from.len());
        effective_prompt = format!("{block}\n\n{effective_prompt}");
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

pub(super) fn inject_skill(prompt: &str, requested_skills: &[String]) -> Result<String> {
    if requested_skills.is_empty() { return Ok(prompt.to_string()); }
    let skill_text = skills::load_skills(requested_skills)?;
    Ok(format!("{prompt}\n\n--- Methodology ---\n{skill_text}"))
}

fn output_file_instruction() -> String {
    "IMPORTANT: Your final response will be saved to a file. Write ONLY the requested deliverable content in your final response. Do NOT include planning, reasoning, chain-of-thought, or meta-commentary. The file should contain only the finished work product.".to_string()
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
    store.get_workgroup(group_id)?.ok_or_else(|| anyhow::anyhow!("Workgroup '{}' not found", group_id)).map(Some)
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
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;
    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id, None).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out, workgroup_id).await?
    };
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

pub(super) fn maybe_cleanup_fast_fail_impl(store: &Store, task_id: &TaskId, task: &Task) {
    let Some(ref wt_path) = task.worktree_path else { return };
    // SANDBOX: refuse to touch anything outside /tmp/aid-wt-*
    if !crate::cmd::merge::merge_git::is_safe_worktree_path(wt_path) {
        eprintln!("[aid] SAFETY: refusing to remove '{}' — not an aid worktree path", wt_path);
        return;
    }
    let path = std::path::Path::new(wt_path);
    if !path.exists() { return }
    let Some(task) = store.get_task(task_id.as_str()).ok().flatten() else { return };
    if task.status != TaskStatus::Failed { return }
    let Some(duration_ms) = task.duration_ms else { return };
    if duration_ms > 10_000 { return }
    if crate::worktree::branch_has_commits_ahead_of_main(path, task.worktree_branch.as_deref().unwrap_or("unknown")).unwrap_or(true) { return; }
    let Some(repo_dir) = task.repo_path.as_deref() else {
        eprintln!("[aid] Warning: skipping fast-fail cleanup for {} — missing repo_path", task_id);
        return;
    };
    let _ = std::process::Command::new("git")
        .args(["-C", repo_dir, "worktree", "remove", "--force", wt_path])
        .output();
    eprintln!("[aid] Cleaned up worktree for fast-failed task {}", task_id);
}

pub(super) fn maybe_verify_impl(store: &Store, task_id: &TaskId, verify: Option<&str>, dir: Option<&str>) {
    let Some(verify_arg) = verify else { return };
    let Some(dir_path) = dir else { println!("Verify skipped: no working directory"); return; };
    let command = if verify_arg == "auto" { None } else { Some(verify_arg) };
    let path = std::path::Path::new(dir_path);
    let worktree_branch = store
        .get_task(task_id.as_str())
        .ok()
        .flatten()
        .and_then(|task| task.worktree_branch);
    let cargo_target_dir = crate::agent::target_dir_for_worktree(worktree_branch.as_deref());
    match crate::verify::run_verify(path, command, cargo_target_dir.as_deref()) {
        Ok(result) => {
            let report = crate::verify::format_verify_report(&result);
            println!("{report}");
            crate::verify::record_verify_status(store, task_id, &result);
            if !result.success {
                let event = TaskEvent { task_id: task_id.clone(), timestamp: Local::now(), event_kind: EventKind::Error, detail: format!("Verification failed: {}", result.command), metadata: None };
                let _ = store.insert_event(&event);
            }
        }
        Err(e) => eprintln!("Verify error: {e}"),
    }
}

pub(super) async fn maybe_auto_retry_after_verify_failure_impl(
    store: &Arc<Store>,
    task_id: &TaskId,
    args: &RunArgs,
    pre_verify_status: TaskStatus,
) -> Result<Option<TaskId>> {
    if args.verify.is_none() || args.retry == 0 || pre_verify_status != TaskStatus::Done {
        return Ok(None);
    }
    let Some(task) = store.get_task(task_id.as_str())? else { return Ok(None) };
    if task.verify_status != crate::types::VerifyStatus::Failed {
        return Ok(None);
    }

    eprintln!(
        "[aid] Verify failed, auto-retrying ({} retries left)",
        args.retry - 1
    );

    let mut retry_args = args.clone();
    retry_args.prompt = format!(
        "[Previous attempt feedback]\n{VERIFY_RETRY_FEEDBACK}\n\n[Original task]\n{}",
        task.prompt
    );
    retry_args.retry = args.retry.saturating_sub(1);
    retry_args.parent_task_id = Some(task_id.as_str().to_string());
    retry_args.repo = task.repo_path.clone().or_else(|| retry_args.repo.clone());
    retry_args.output = task
        .output_path
        .clone()
        .or_else(|| retry_args.output.clone());
    retry_args.model = task.model.clone().or_else(|| retry_args.model.clone());
    retry_args.verify = task.verify.clone();
    retry_args.read_only = task.read_only;
    retry_args.budget = task.budget;
    retry_args.background = false;
    let (dir, worktree) = retry_target(&task);
    retry_args.dir = dir.or_else(|| retry_args.dir.clone());
    retry_args.worktree = worktree.or_else(|| retry_args.worktree.clone());
    if task.agent == AgentKind::OpenCode {
        retry_args.session_id = task.agent_session_id.clone();
    }

    Box::pin(super::run(store.clone(), retry_args)).await.map(Some)
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
    eprintln!("[aid] Compacted prompt from ~{before} to ~{after} tokens");
    result
}

pub(super) fn warn_agent_committed_files_outside_scope(
    scope: &[String],
    dir: Option<&String>,
    effective_dir: Option<&String>,
    resolved_repo: Option<&String>,
    worktree_path: Option<&String>,
) {
    if scope.is_empty() && dir.map(|value| value.trim()).unwrap_or("").is_empty() {
        return;
    }
    let base_path = worktree_path
        .map(PathBuf::from)
        .or_else(|| effective_dir.map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("."));
    let changed_files = match worktree::worktree_changed_files(&base_path) {
        Ok(files) if !files.is_empty() => files,
        _ => return,
    };
    let base_dir = base_path.to_string_lossy().to_string();
    let repo_root = resolved_repo
        .map(PathBuf::from)
        .or_else(|| resolve_repo_path(&base_dir).ok().map(PathBuf::from));
    let scope_paths = normalized_scope_paths(scope, repo_root.as_deref());
    let dir_path = normalized_dir_path(dir, repo_root.as_deref());
    if scope_paths.is_empty() && dir_path.is_none() {
        return;
    }
    let mut violations = Vec::new();
    for file in changed_files {
        let file_path = Path::new(&file);
        let scope_violation = !scope_paths.is_empty()
            && !scope_paths
                .iter()
                .any(|scope| file_path == scope || file_path.starts_with(scope));
        let dir_violation = dir_path
            .as_ref()
            .is_some_and(|dir| !(file_path == dir || file_path.starts_with(dir)));
        if scope_violation || dir_violation {
            violations.push(file);
        }
    }
    if violations.is_empty() {
        return;
    }
    eprintln!(
        "[aid] Warning: agent committed {} files outside scope: {:?}",
        violations.len(),
        violations
    );
}

fn normalized_scope_paths(scope: &[String], repo_root: Option<&Path>) -> Vec<PathBuf> {
    scope
        .iter()
        .filter_map(|entry| {
            let trimmed = entry.trim().trim_end_matches('/');
            if trimmed.is_empty() {
                return None;
            }
            let path = Path::new(trimmed);
            let relative = if path.is_absolute() {
                let root = repo_root?;
                path.strip_prefix(root).ok()?
            } else {
                path
            };
            let normalized = normalize_relative_path(relative);
            if normalized.as_os_str().is_empty() {
                return None;
            }
            Some(normalized)
        })
        .collect()
}

fn normalized_dir_path(dir: Option<&String>, repo_root: Option<&Path>) -> Option<PathBuf> {
    let dir = dir?;
    let trimmed = dir.trim().trim_end_matches('/');
    if trimmed.is_empty() || trimmed == "." {
        return None;
    }
    let path = Path::new(trimmed);
    let relative = if path.is_absolute() {
        let root = repo_root?;
        path.strip_prefix(root).ok()?
    } else {
        path
    };
    let normalized = normalize_relative_path(relative);
    if normalized.as_os_str().is_empty() {
        return None;
    }
    Some(normalized)
}

fn normalize_relative_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => {
                normalized.pop();
            }
            Component::Normal(part) => normalized.push(part),
            _ => normalized.push(component.as_os_str()),
        }
    }
    normalized
}

#[cfg(test)]
#[path = "run_prompt/tests.rs"]
mod tests;

#[cfg(test)]
mod sanitize_tests {
    use super::sanitize_injected_text;

    #[test]
    fn sanitize_strips_structural_tags() {
        let input = "keep\n<aid-project-rules>\ninside\n</aid-team-rules>\nend";
        let sanitized = sanitize_injected_text(input);
        assert_eq!(sanitized, "keep\ninside\nend");
    }

    #[test]
    fn sanitize_preserves_normal_lines() {
        let input = "alpha\n beta\n[Task]\nplain text";
        let sanitized = sanitize_injected_text(input);
        assert_eq!(sanitized, input);
    }
}
