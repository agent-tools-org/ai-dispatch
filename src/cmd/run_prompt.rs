// Prompt and run helpers for `aid run`.
// Exports: build_prompt_bundle(), resolve_prompt(), build_context_flags(), run_agent_process_impl().
// Deps: context, templates, workgroup, skills, watcher, store.
use anyhow::{Context, Result};
use chrono::Local;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use tokio::process::Command;
use crate::{agent, skills, store::Store, templates, team, types::*, watcher};
use crate::team::KnowledgeEntry;
use super::RunArgs;

const VERIFY_RETRY_FEEDBACK: &str =
    "Verification failed. Please fix the compilation/test errors and try again.";
const PROMPT_TOKEN_LIMIT: usize = 30_000;

pub(super) struct PromptBundle { pub effective_prompt: String, pub context_files: Vec<String>, pub prompt_tokens: i64, pub injected_memory_ids: Vec<String> }

#[allow(dead_code)]
pub(super) enum PromptSource<'a> { Inline(&'a str), File(&'a str) }

pub(super) fn build_prompt_bundle(store: &Store, args: &RunArgs, agent_kind: &AgentKind, workgroup: Option<&Workgroup>, requested_skills: &[String]) -> Result<PromptBundle> {
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
    let prompt = resolve_prompt(PromptSource::Inline(&args.prompt), args.template.as_deref())?;
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
    let mut effective_prompt = inject_skill(&effective_prompt, requested_skills)?;
    let mut injected_memory_ids = Vec::new();

    // Inject relevant memories from past tasks
    if let Some((memory_block, memory_ids)) = inject_memories(store, &args.prompt, 10)? {
        effective_prompt = format!("{memory_block}\n\n{effective_prompt}");
        injected_memory_ids = memory_ids;
    }

    // Inject EverMemOS cloud memories (semantic search)
    if let Some(cloud_block) = inject_evermemos_memories(&args.prompt)? {
        effective_prompt = format!("{cloud_block}\n\n{effective_prompt}");
    }

    // Inject team knowledge if --team was specified
    if let Some(ref team_id) = args.team {
        let entries = team::read_knowledge_entries(team_id);
        let total_entries = entries.len();
        if total_entries > 0 {
            let relevant = select_relevant_entries(&entries, &args.prompt);
            eprintln!("[aid] Injected {}/{} knowledge entries (relevance-filtered)", relevant.len(), total_entries);
            if !relevant.is_empty() {
                let knowledge_block = format_knowledge_block(team_id, &relevant);
                effective_prompt = format!("{knowledge_block}\n\n{effective_prompt}");
            }
        }
    }

    // Inject output from previous tasks (--context-from)
    if !args.context_from.is_empty() {
        if let Some(block) = resolve_context_from(store, &args.context_from)? {
            let token_count = templates::estimate_tokens(&block);
            eprintln!("[aid] Injected context from {} task(s) (~{token_count} tokens)", args.context_from.len());
            effective_prompt = format!("{block}\n\n{effective_prompt}");
        }
    }

    // Inject workspace path if workgroup has one
    if let Some(ref group_id) = args.group {
        let workspace = crate::paths::workspace_dir(group_id);
        if workspace.is_dir() {
            effective_prompt = format!(
                "[Shared Workspace]\nPath: {}\nUse this directory for intermediate artifacts, shared files, and inter-agent communication.\n\n{effective_prompt}",
                workspace.display()
            );
        }
    }

    // Compact prompt if it exceeds token budget
    let effective_prompt = maybe_compact_prompt(&effective_prompt, PROMPT_TOKEN_LIMIT);
    let prompt_tokens = templates::estimate_tokens(&effective_prompt) as i64;
    Ok(PromptBundle { effective_prompt, context_files, prompt_tokens, injected_memory_ids })
}

/// Query relevant memories and inject them into the prompt.
fn inject_memories(store: &Store, prompt: &str, max_memories: usize) -> Result<Option<(String, Vec<String>)>> {
    let project_path = detect_project_path();
    let keywords = extract_words(prompt);
    let queries = build_memory_queries(prompt, &keywords);
    if queries.is_empty() {
        return Ok(None);
    }

    let mut scored: HashMap<String, (Memory, usize)> = HashMap::new();
    for query in queries {
        for memory in store.search_memories(&query, project_path.as_deref(), max_memories)? {
            let entry = scored
                .entry(memory.id.as_str().to_string())
                .or_insert((memory.clone(), 0));
            entry.1 += 1;
        }
    }

    let mut scored: Vec<_> = scored.into_values().collect();
    scored.sort_by(|a, b| {
        b.1.cmp(&a.1)
            .then_with(|| b.0.created_at.cmp(&a.0.created_at))
    });
    let memories: Vec<_> = scored.into_iter().take(max_memories).map(|(mem, _)| mem).collect();
    if memories.is_empty() {
        return Ok(None);
    }
    for memory in &memories {
        store.increment_memory_inject(memory.id.as_str())?;
    }

    let mut lines = vec!["[Agent Memory — knowledge from past tasks]".to_string()];
    let now = Local::now();
    for mem in &memories {
        let age = format_memory_age(now.signed_duration_since(mem.created_at));
        lines.push(format!("- [{}] ({}) {}", mem.memory_type.label(), age, mem.content));
    }
    let memory_ids = memories.iter().map(|mem| mem.id.as_str().to_string()).collect();
    let token_count = crate::templates::estimate_tokens(&lines.join("\n"));
    eprintln!("[aid] Injected {} memories (~{} tokens)", memories.len(), token_count);
    Ok(Some((lines.join("\n"), memory_ids)))
}

fn inject_evermemos_memories(prompt: &str) -> Result<Option<String>> {
    let config = crate::config::load_config()?;
    let client = match crate::evermemos::EverMemosClient::from_config(&config.evermemos) {
        Some(c) => c,
        None => return Ok(None),
    };
    let memories = match client.search_memories(prompt, 5) {
        Ok(mems) => mems,
        Err(e) => {
            eprintln!("[aid] EverMemOS search failed: {e}");
            return Ok(None);
        }
    };
    if memories.is_empty() {
        return Ok(None);
    }
    let mut lines = vec!["[Cloud Memory — EverMemOS semantic recall]".to_string()];
    for mem in &memories {
        lines.push(format!("- (score:{:.2}) {}", mem.score, mem.content));
    }
    let block = lines.join("\n");
    let token_count = crate::templates::estimate_tokens(&block);
    eprintln!("[aid] Injected {} cloud memories (~{} tokens)", memories.len(), token_count);
    Ok(Some(block))
}

fn build_memory_queries(prompt: &str, keywords: &HashSet<String>) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut queries = Vec::new();

    let trimmed = prompt.trim();
    if !trimmed.is_empty() {
        let truncated = trimmed.chars().take(200).collect::<String>();
        if seen.insert(truncated.clone()) {
            queries.push(truncated);
        }
    }

    let top_words = top_significant_words(prompt, keywords, 5);
    if !top_words.is_empty() {
        let joined = top_words.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    let paths = extract_path_tokens(prompt);
    if !paths.is_empty() {
        let joined = paths.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    let idents = extract_type_or_function_names(prompt);
    if !idents.is_empty() {
        let joined = idents.join(" ");
        if seen.insert(joined.clone()) {
            queries.push(joined);
        }
    }

    queries
}

fn top_significant_words(text: &str, keywords: &HashSet<String>, limit: usize) -> Vec<String> {
    let mut counts = HashMap::new();
    for token in text.split(|c: char| !c.is_alphanumeric()) {
        let normalized = token.to_lowercase();
        if normalized.is_empty() || !keywords.contains(&normalized) {
            continue;
        }
        *counts.entry(normalized).or_insert(0) += 1;
    }
    let mut entries: Vec<_> = counts.into_iter().collect();
    entries.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(&b.0)));
    entries.into_iter().take(limit).map(|(word, _)| word).collect()
}

fn extract_path_tokens(prompt: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    prompt
        .split_whitespace()
        .filter_map(|token| {
            let trimmed = token.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '?' | '!' | '"' | '\'' | '[' | ']' | '{' | '}' | '(' | ')'));
            if trimmed.is_empty() || (!trimmed.contains('/') && !trimmed.contains('\\')) {
                return None;
            }
            let candidate = trimmed.to_string();
            if seen.insert(candidate.clone()) { Some(candidate) } else { None }
        })
        .collect()
}

fn extract_type_or_function_names(prompt: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut names = Vec::new();
    for token in prompt.split_whitespace() {
        let trimmed = token.trim_matches(|c: char| matches!(c, ',' | ';' | '.' | '?' | '!' | '"' | '\'' | '[' | ']' | '{' | '}'));
        if trimmed.is_empty() {
            continue;
        }
        let key = trimmed.strip_suffix("()").unwrap_or(trimmed).to_string();
        let qualifies = trimmed.contains("::") || trimmed.ends_with("()") || key.chars().any(|c| c.is_ascii_uppercase());
        if qualifies && seen.insert(key.clone()) {
            names.push(key);
        }
    }
    names
}

fn format_memory_age(duration: chrono::Duration) -> String {
    let days = duration.num_days();
    if days >= 30 { format!("{}mo ago", days / 30) }
    else if days >= 1 { format!("{}d ago", days) }
    else {
        let hours = duration.num_hours();
        if hours >= 1 { format!("{}h ago", hours) }
        else { format!("{}m ago", duration.num_minutes().max(1)) }
    }
}

fn format_knowledge_block(team_id: &str, entries: &[&KnowledgeEntry]) -> String {
    let blocks: Vec<String> = entries.iter().map(|entry| format_entry_block(entry)).collect();
    format!("[Team Knowledge — {team_id}]\n{}", blocks.join("\n\n"))
}

fn format_entry_block(entry: &KnowledgeEntry) -> String {
    let mut line = String::new();
    line.push_str("- [");
    line.push_str(&entry.topic);
    line.push(']');
    if let Some(path) = &entry.path {
        line.push('(');
        line.push_str(path);
        line.push(')');
    }
    line.push_str(" — ");
    line.push_str(&entry.description);
    if let Some(content) = &entry.content {
        line.push('\n');
        line.push_str(content);
    }
    line
}

fn select_relevant_entries<'a>(entries: &'a [KnowledgeEntry], prompt: &str) -> Vec<&'a KnowledgeEntry> {
    let prompt_words = extract_words(prompt);
    let mut scored: Vec<(usize, &KnowledgeEntry)> = entries
        .iter()
        .map(|entry| {
            let entry_words = extract_words(&format!("{} {}", entry.topic, entry.description));
            let score = entry_words.iter().filter(|word| prompt_words.contains(*word)).count();
            (score, entry)
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0));
    scored
        .into_iter()
        .filter(|(score, _)| *score > 0)
        .take(5)
        .map(|(_, entry)| entry)
        .collect()
}

fn extract_words(value: &str) -> HashSet<String> {
    value
        .split(|c: char| !c.is_alphanumeric())
        .filter_map(|token| {
            let normalized = token.to_lowercase();
            if normalized.is_empty() { None } else { Some(normalized) }
        })
        .collect()
}

fn detect_project_path() -> Option<String> {
    std::process::Command::new("git")
        .args(["rev-parse", "--show-toplevel"])
        .output()
        .ok()
        .and_then(|o| if o.status.success() {
            String::from_utf8(o.stdout).ok().map(|s| s.trim().to_string())
        } else {
            None
        })
}

/// Resolve --context-from task IDs: read output/diff from completed tasks.
fn resolve_context_from(store: &Store, task_ids: &[String]) -> Result<Option<String>> {
    let mut blocks = Vec::new();
    for task_id in task_ids {
        let Some(task) = store.get_task(task_id)? else {
            eprintln!("[aid] Warning: --context-from task '{task_id}' not found, skipping");
            continue;
        };
        let mut content = String::new();
        // Try output file first
        if let Some(ref path) = task.output_path {
            if let Ok(text) = std::fs::read_to_string(path) {
                content = text;
            }
        }
        // Fall back to log file
        if content.is_empty() {
            if let Some(ref log_path) = task.log_path {
                if let Ok(text) = std::fs::read_to_string(log_path) {
                    // Take last 200 lines to avoid huge context
                    let lines: Vec<&str> = text.lines().collect();
                    let start = lines.len().saturating_sub(200);
                    content = lines[start..].join("\n");
                }
            }
        }
        if content.is_empty() {
            eprintln!("[aid] Warning: --context-from task '{task_id}' has no output, skipping");
            continue;
        }
        blocks.push(format!(
            "[Prior Task Result — {} ({}, {})]\n{}",
            task_id,
            task.agent_display_name(),
            task.status.as_str(),
            content.trim()
        ));
    }
    if blocks.is_empty() {
        return Ok(None);
    }
    Ok(Some(blocks.join("\n\n")))
}

pub(super) fn resolve_prompt(source: PromptSource<'_>, template: Option<&str>) -> Result<String> {
    let raw = match source { PromptSource::Inline(prompt) => prompt.to_string(), PromptSource::File(path) => read_context_file(path)?, };
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
pub(super) fn resolve_worktree_paths(args: &RunArgs, repo_path: Option<&str>) -> Result<(Option<String>, Option<String>, Option<String>, Option<String>)> {
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

pub(super) async fn run_agent_process_impl(
    agent: &dyn crate::agent::Agent,
    mut cmd: Command,
    task_id: &TaskId,
    store: &Arc<Store>,
    log_path: &std::path::Path,
    output_path: Option<&str>,
    model: Option<&str>,
    streaming: bool,
    workgroup_id: Option<&str>,
) -> Result<()> {
    let start = std::time::Instant::now();
    let mut child = cmd.spawn().context("Failed to spawn agent process")?;
    let info = if streaming {
        watcher::watch_streaming(agent, &mut child, task_id, store, log_path, workgroup_id).await?
    } else {
        let out = output_path.map(std::path::Path::new);
        watcher::watch_buffered(agent, &mut child, task_id, store, log_path, out, workgroup_id).await?
    };
    let duration_ms = start.elapsed().as_millis() as i64;
    let final_model = info.model.as_deref().or(model);
    let cost_usd = info.cost_usd.or_else(|| info.tokens.and_then(|tokens| crate::cost::estimate_cost(tokens, final_model, agent.kind())));
    store.update_task_completion(task_id.as_str(), info.status, info.tokens, duration_ms, final_model, cost_usd, info.exit_code)?;
    let duration_str = format_duration(duration_ms);
    let tokens_str = info.tokens.map(|t| format!(", {} tokens", t)).unwrap_or_default();
    let cost_str = if cost_usd.is_some() { format!(", {}", crate::cost::format_cost(cost_usd)) } else { String::new() };
    println!("Task {} {} ({}{}{})", task_id, info.status.label(), duration_str, tokens_str, cost_str);
    Ok(())
}

pub(super) fn maybe_cleanup_fast_fail_impl(store: &Store, task_id: &TaskId, task: &Task) {
    let Some(ref wt_path) = task.worktree_path else { return };
    let path = std::path::Path::new(wt_path);
    if !path.exists() { return }
    let Some(task) = store.get_task(task_id.as_str()).ok().flatten() else { return };
    if task.status != TaskStatus::Failed { return }
    let Some(duration_ms) = task.duration_ms else { return };
    if duration_ms > 10_000 { return }
    if crate::worktree::branch_has_commits_ahead_of_main(path, task.worktree_branch.as_deref().unwrap_or("unknown")).unwrap_or(true) { return; }
    let _ = std::process::Command::new("git").args(["worktree", "remove", "--force", wt_path]).output();
    eprintln!("[aid] Cleaned up worktree for fast-failed task {}", task_id);
}

pub(super) fn maybe_verify_impl(store: &Store, task_id: &TaskId, verify: Option<&str>, dir: Option<&str>) {
    let Some(verify_arg) = verify else { return };
    let Some(dir_path) = dir else { println!("Verify skipped: no working directory"); return; };
    let command = if verify_arg == "auto" { None } else { Some(verify_arg) };
    let path = std::path::Path::new(dir_path);
    match crate::verify::run_verify(path, command) {
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
    if let Some(task) = store.get_task(task_id.as_str())? { crate::notify::notify_completion(&task); }
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

#[cfg(test)]
#[path = "run_prompt/tests.rs"]
mod tests;
