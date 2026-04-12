// `aid run` argument assembly helpers for the primary dispatch handlers.
// Exports: build_run_args().
// Deps: crate::cli::RunExtrasArgs, crate::cmd::run::RunArgs, idle timeout helpers.

use crate::cli::RunExtrasArgs;
use crate::cmd;
use crate::cmd_dispatch::resolve_group;

#[allow(clippy::too_many_arguments)]
pub(super) fn build_run_args(
    agent_name: String,
    prompt: String,
    prompt_file: Option<String>,
    repo: Option<String>,
    repo_root: Option<String>,
    dir: Option<String>,
    output: Option<String>,
    result_file: Option<String>,
    model: Option<String>,
    auto_model: Option<String>,
    worktree: Option<String>,
    group: Option<String>,
    verify: Option<String>,
    iterate: Option<u32>,
    eval: Option<String>,
    eval_feedback_template: Option<String>,
    judge: Option<String>,
    peer_review: Option<String>,
    retry: u32,
    context: Vec<String>,
    checklist: Vec<String>,
    scope: Vec<String>,
    run_extras: Box<RunExtrasArgs>,
    no_skill: bool,
    bg: bool,
    dry_run: bool,
    read_only: bool,
    sandbox: bool,
    container: Option<String>,
    budget: bool,
    best_of: Option<usize>,
    metric: Option<String>,
    team_flag: Option<String>,
    parent: Option<String>,
    id: Option<String>,
    timeout: Option<u64>,
    idle_timeout: Option<u64>,
    audit: bool,
    no_audit: bool,
    no_link_deps: bool,
) -> cmd::run::RunArgs {
    let extras = *run_extras;
    let skills = if no_skill {
        vec![cmd::run::NO_SKILL_SENTINEL.to_string()]
    } else {
        extras.skill
    };
    let effective_idle_timeout =
        idle_timeout.or_else(|| crate::agent_config::get_default_idle_timeout(&agent_name));
    let env = crate::idle_timeout::env_with_idle_timeout(None, effective_idle_timeout);

    cmd::run::RunArgs {
        agent_name,
        prompt,
        prompt_file,
        repo,
        repo_root,
        dir,
        output,
        result_file,
        model: model.or(auto_model),
        worktree,
        base_branch: None,
        group: resolve_group(group),
        verify,
        iterate,
        eval,
        eval_feedback_template,
        judge,
        peer_review,
        max_duration_mins: None,
        retry,
        context,
        checklist,
        skills,
        hooks: extras.hook,
        template: extras.template,
        background: bg,
        dry_run,
        announce: true,
        on_done: extras.on_done,
        cascade: extras.cascade,
        read_only,
        sandbox,
        container,
        budget,
        best_of,
        metric,
        team: team_flag,
        context_from: extras.context_from,
        scope,
        parent_task_id: parent,
        env,
        existing_task_id: id.map(crate::types::TaskId),
        timeout,
        audit,
        audit_explicit: audit,
        no_audit,
        link_deps: !no_link_deps,
        ..Default::default()
    }
}
