// Batch task -> run args conversion helpers.
// Exports: task_to_run_args
// Deps: crate::cmd::run::RunArgs, crate::batch, crate::store::Store
use crate::batch;
use crate::agent::model_validation::ModelSource;
use crate::cmd::run::{RunArgs, NO_SKILL_SENTINEL};
use crate::store::Store;
use std::collections::HashMap;
use std::sync::Arc;

pub(crate) fn task_to_run_args(
    task: &batch::BatchTask,
    siblings: &[&batch::BatchTask],
    background: bool,
    store: &Arc<Store>, // retained for call-site stability; selection no longer reads it
    shared_dir_path: Option<&str>,
) -> RunArgs {
    let _ = store;
    // Empty/`auto` agents are rejected in batch validation; require an explicit name.
    let agent_name = task.agent.clone();
    let batch_siblings = siblings
        .iter()
        .map(|sibling| {
            (
                sibling
                    .name
                    .clone()
                    .or_else(|| sibling.id.clone())
                    .unwrap_or_else(|| "<unnamed>".to_string()),
                sibling.agent.clone(),
                sibling.prompt.clone(),
            )
        })
        .collect();
    let cascade = task
        .fallback
        .as_deref()
        .map(|fallback| {
            fallback
                .split(',')
                .map(str::trim)
                .filter(|agent| !agent.is_empty())
                .map(ToString::to_string)
                .collect()
        })
        .unwrap_or_else(|| auto_cascade_for_rate_limited(&agent_name, &task.prompt));
    let env = merged_env(task.env.as_ref(), task.env_forward.as_ref(), shared_dir_path);
    let skills = if task.no_skill {
        vec![NO_SKILL_SENTINEL.to_string()]
    } else {
        task.skills.clone().unwrap_or_default()
    };
    let profile_model = crate::agent::selection::resolve_explicit_agent_model(
        &agent_name,
        task.model.as_deref(),
        task.budget,
        false,
    );
    let model_source = if task.model.is_some() {
        ModelSource::UserSupplied
    } else {
        ModelSource::AidResolved
    };
    RunArgs {
        agent_name,
        prompt: task.prompt.clone(),
        dir: task.dir.clone(),
        output: task.output.clone(),
        result_file: auto_scope_result_file(task, siblings),
        model: task.model.clone().or(profile_model),
        model_source,
        declared_difficulty: task.difficulty,
        declared_budget: task.budget,
        declared_urgency: task.urgency,
        declared_rigor: task.rigor,
        declared_egress: task.egress.unwrap_or_default(),
        kind: task.kind,
        worktree: task.worktree.clone(),
        group: task.group.clone(),
        container: task.container.clone(),
        verify: task.verify.clone(),
        setup: task.setup.clone(),
        iterate: task.iterate,
        eval: task.eval.clone(),
        eval_feedback_template: task.eval_feedback_template.clone(),
        judge: task.judge.clone(),
        peer_review: task.peer_review.clone(),
        max_duration_mins: task.max_duration_mins.map(|value| value as i64),
        retry: task.retry.unwrap_or(0),
        context: task.context.clone().unwrap_or_default(),
        checklist: task.checklist.clone().unwrap_or_default(),
        skills,
        hooks: task.hooks.clone().unwrap_or_default(),
        background,
        dry_run: false,
        announce: true,
        on_done: task.on_done.clone(),
        cascade,
        read_only: task.read_only,
        sandbox: task.sandbox,
        budget: task.budget.is_some_and(crate::types::TaskBudget::uses_budget_mode),
        best_of: task.best_of,
        metric: task.metric.clone(),
        team: task.team.clone(),
        context_from: task.context_from.clone().unwrap_or_default(),
        batch_siblings,
        scope: task.scope.clone().unwrap_or_default(),
        parent_task_id: task.parent.clone(),
        existing_task_id: task.id.as_ref().map(|id| crate::types::TaskId(id.clone())),
        env,
        env_forward: task.env_forward.clone(),
        idle_timeout_secs: task.idle_timeout,
        audit: task.audit.unwrap_or(false),
        audit_explicit: task.audit.is_some(),
        link_deps: task.worktree_link_deps.unwrap_or(true),
        ..Default::default()
    }
}

/// If the agent is rate-limited, return the suggested fallback as an auto-cascade.
fn auto_cascade_for_rate_limited(agent_name: &str, prompt: &str) -> Vec<String> {
    let (agent, custom_name) = crate::rate_limit::resolve_agent(agent_name);
    if !crate::rate_limit::is_rate_limited(&agent, custom_name) {
        return vec![];
    }
    // Coding fallback is defined for built-ins; a held custom has no matrix entry.
    if agent == crate::types::AgentKind::Custom {
        return vec![];
    }
    crate::agent::selection::coding_fallback_for_prompt(&agent, prompt)
        .map(|fallback| vec![fallback.as_str().to_string()])
        .unwrap_or_default()
}

/// Auto-scope result_file when sibling tasks share the same filename.
/// Appends `-{task_name}` before the extension to prevent parallel overwrites.
fn auto_scope_result_file(task: &batch::BatchTask, siblings: &[&batch::BatchTask]) -> Option<String> {
    let result_file = task.result_file.as_deref()?;
    let has_collision = siblings.iter().any(|s| s.result_file.as_deref() == Some(result_file));
    if !has_collision {
        return Some(result_file.to_string());
    }
    let task_name = task.name.as_deref()
        .or(task.id.as_deref())
        .unwrap_or("task");
    let scoped = scope_filename(result_file, task_name);
    aid_info!("[aid] Auto-scoped result_file: {result_file} → {scoped} (collision with sibling)");
    Some(scoped)
}

fn scope_filename(path: &str, suffix: &str) -> String {
    match path.rsplit_once('.') {
        Some((stem, ext)) => format!("{stem}-{suffix}.{ext}"),
        None => format!("{path}-{suffix}"),
    }
}

fn merged_env(
    env: Option<&HashMap<String, String>>,
    env_forward: Option<&Vec<String>>,
    shared_dir_path: Option<&str>,
) -> Option<HashMap<String, String>> {
    let mut merged = env.cloned().unwrap_or_default();
    if let Some(shared_dir_path) = shared_dir_path {
        merged.insert("AID_SHARED_DIR".to_string(), shared_dir_path.to_string());
    }
    if let Some(env_forward) = env_forward {
        for name in env_forward {
            if let Ok(value) = std::env::var(name) {
                merged.insert(name.clone(), value);
            }
        }
    }
    (!merged.is_empty()).then_some(merged)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn budget_selected_batch_model_is_aid_resolved() {
        let home = tempfile::tempdir().expect("temporary aid home");
        let _guard = crate::paths::AidHomeGuard::set(home.path());
        let task: batch::BatchTask = toml::from_str(
            r#"
            agent = "qwen"
            prompt = "say hi"
            budget = "cheap"
            "#,
        )
        .expect("valid batch task");
        let store = Arc::new(Store::open_memory().expect("in-memory store"));

        let args = task_to_run_args(&task, &[], false, &store, None);

        assert!(args.model.is_some(), "budget should select a model");
        assert_eq!(args.model_source, ModelSource::AidResolved);
    }

    fn batch_task(toml: &str) -> (tempfile::TempDir, crate::paths::AidHomeGuard, batch::BatchTask) {
        let home = tempfile::tempdir().expect("temporary aid home");
        let guard = crate::paths::AidHomeGuard::set(home.path());
        let task: batch::BatchTask = toml::from_str(toml).expect("valid batch task");
        (home, guard, task)
    }

    #[test]
    fn configured_default_outranks_declared_batch_budget() {
        let (_home, _guard, task) = batch_task(
            r#"
            agent = "gemini"
            prompt = "say hi"
            budget = "cheap"
            "#,
        );
        crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("save config");
        let store = Arc::new(Store::open_memory().expect("in-memory store"));

        let args = task_to_run_args(&task, &[], false, &store, None);

        assert_eq!(
            args.model.as_deref(),
            Some("pro"),
            "batch must match run: configured default outranks catalog cheap pick flash-lite"
        );
        assert_eq!(args.model_source, ModelSource::AidResolved);
    }

    #[test]
    fn batch_task_model_beats_configured_default() {
        let (_home, _guard, task) = batch_task(
            r#"
            agent = "gemini"
            prompt = "say hi"
            budget = "cheap"
            model = "flash"
            "#,
        );
        crate::agent_config::save_agent_default_model("gemini", Some("pro")).expect("save config");
        let store = Arc::new(Store::open_memory().expect("in-memory store"));

        let args = task_to_run_args(&task, &[], false, &store, None);

        assert_eq!(args.model.as_deref(), Some("flash"));
        assert_eq!(args.model_source, ModelSource::UserSupplied);
    }

    #[test]
    fn batch_uses_catalog_when_no_default_is_configured() {
        let (_home, _guard, task) = batch_task(
            r#"
            agent = "gemini"
            prompt = "say hi"
            budget = "cheap"
            "#,
        );
        let store = Arc::new(Store::open_memory().expect("in-memory store"));

        let args = task_to_run_args(&task, &[], false, &store, None);

        assert_eq!(args.model.as_deref(), Some("flash-lite"));
        assert_eq!(args.model_source, ModelSource::AidResolved);
    }
}
