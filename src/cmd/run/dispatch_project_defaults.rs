// Apply project defaults before route resolution and task persistence.
// Keeps CLI overrides intact; depends on RunArgs and ProjectConfig.
use super::RunArgs;
use crate::config::project::ProjectConfig;

pub(crate) fn apply_project_defaults(args: &mut RunArgs, detected_project: Option<&ProjectConfig>) {
    if let Some(project) = detected_project {
        let mut defaults_applied = false;
        if args.max_task_cost.is_none() {
            args.max_task_cost = project.max_task_cost;
        }
        if args.team.is_none()
            && let Some(team) = project.team.as_ref()
        {
            args.team = Some(team.clone());
            defaults_applied = true;
        }
        if (args.verify.is_none() || args.verify.as_deref() == Some("auto"))
            && let Some(verify) = project.verify.as_ref()
        {
            args.verify = Some(verify.clone());
            defaults_applied = true;
        }
        if args.setup.is_none()
            && let Some(setup) = project.setup.as_ref()
        {
            args.setup = Some(setup.clone());
            defaults_applied = true;
        }
        args.remote_build = args.remote_build.take().or_else(|| {
            crate::runtime::remote_build::project_default(project.remote_build.as_deref())
        });
        if args.container.is_none()
            && let Some(container) = project.container.as_ref()
        {
            args.container = Some(container.clone());
            defaults_applied = true;
        }
        if args.policy.is_none() && !args.budget && project.budget.prefer_budget {
            args.budget = true;
            defaults_applied = true;
        }
        if args.no_audit || args.policy.is_some() {
            args.audit = false;
        } else if !args.audit_explicit && project.audit_auto() {
            args.audit = true;
            defaults_applied = true;
        }
        if defaults_applied {
            announce_project_defaults(args, project);
        }
    }
}

fn announce_project_defaults(args: &RunArgs, project: &ProjectConfig) {
    aid_info!(
        "[aid] Project '{}' defaults: team={}, verify={}, audit={}",
        project.id,
        args.team.as_deref().unwrap_or("None"),
        args.verify.as_deref().unwrap_or("None"),
        if args.audit { "on" } else { "off" },
    );
}
