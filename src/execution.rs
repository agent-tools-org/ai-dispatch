// aid.execution/1 capability discovery and structured protocol failures.
// Shared by CLI admission, dispatch and execution receipts.
use anyhow::Result;
use serde_json::{Value, json};

pub(crate) const PROTOCOL: &str = "aid.execution/1";
pub(crate) const REFUSED: &[&str] = &[
    "retry",
    "cascade",
    "best_of",
    "iterate",
    "judge",
    "peer_review",
    "audit",
    "budget",
    "dry_run",
    "container",
    "on_done",
    "hook",
];

#[derive(Debug)]
pub(crate) struct Refusal {
    pub code: &'static str,
    pub detail: Option<String>,
    pub exit: i32,
}

impl std::fmt::Display for Refusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "{}{}",
            self.code,
            self.detail.as_ref().map(|s| format!(": {s}")).unwrap_or_default()
        )
    }
}
impl std::error::Error for Refusal {}

pub(crate) fn refuse(code: &'static str, detail: Option<&str>, exit: i32) -> anyhow::Error {
    Refusal {
        code,
        detail: detail.map(str::to_owned),
        exit,
    }
    .into()
}

impl Refusal {
    pub(crate) fn body(&self) -> Value {
        json!({"protocol": PROTOCOL, "error": {
            "code": self.code, "message": self.to_string(), "detail": self.detail,
        }})
    }
}

pub(crate) fn capabilities() -> Value {
    json!({
        "protocol": PROTOCOL, "aid_version": env!("CARGO_PKG_VERSION"),
        "capabilities": ["caller_owned_policy", "settlement_receipt", "worker_stop", "idempotent_submit"],
        "policies": {"caller-owned": {"version": 1, "requires": ["id", "model"],
            "refused_flags": REFUSED,
            "disabled": ["retry", "cascade", "model_substitution", "route_substitution", "model_self_heal",
                "hang_retry", "delivery_recovery", "dirty_rescue", "quota_continuation",
                "verify_retry", "checklist_retry", "iterate", "best_of", "judge", "peer_review",
                "audit", "nested_delegation", "transport_fallback", "explicit_retry"]}}
    })
}

pub(crate) fn validate(args: &crate::cmd::run::RunArgs) -> Result<()> {
    let Some(policy) = args.policy.as_deref() else {
        return Ok(());
    };
    if policy != "caller-owned" {
        return Err(refuse("policy_unsupported", Some(policy), 3));
    }
    if args
        .existing_task_id
        .as_ref()
        .is_none_or(|id| crate::support::sanitize::validate_task_id(id.as_str()).is_err())
    {
        return Err(refuse("id_required", None, 3));
    }
    if args.model.as_deref().is_none_or(|model| model.trim().is_empty()) {
        return Err(refuse("model_required", None, 3));
    }
    let flags = [
        args.retry > 0,
        !args.cascade.is_empty(),
        args.best_of.is_some(),
        args.iterate.is_some(),
        args.judge.is_some(),
        args.peer_review.is_some(),
        args.audit_explicit,
        args.budget || args.declared_budget.is_some(),
        args.dry_run,
        args.container.is_some(),
        args.on_done.is_some(),
        !args.hooks.is_empty(),
    ];
    for (flag, refused) in REFUSED.iter().zip(flags) {
        if refused {
            return Err(refuse("flag_refused", Some(flag), 3));
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "execution_tests.rs"]
mod tests;

pub(crate) fn validate_cli(cli: &crate::cli::run_args::RunArgs) -> Result<()> {
    validate(&crate::cmd::run::RunArgs {
        policy: cli.policy.clone(),
        dry_run: cli.dry_run,
        container: cli.container.clone(),
        on_done: cli.run_extras.on_done.clone(),
        hooks: cli.run_extras.hook.clone(),
        existing_task_id: cli.id.clone().map(crate::types::TaskId),
        model: cli.model.clone(),
        retry: cli.retry,
        cascade: cli.run_extras.cascade.clone(),
        best_of: cli.best_of,
        iterate: cli.iterate,
        judge: cli.judge.clone(),
        peer_review: cli.peer_review.clone(),
        audit_explicit: cli.audit,
        declared_budget: cli.budget,
        ..Default::default()
    })
}
