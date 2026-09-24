// Advise exclusion reasons and caller-pool gating (weaker same-pool models).
// Exports: Exclusions, CallerAdvice, caller_advice(), PoolVerdict, pool_verdict().
// Deps: provider_for_cli, model catalog capability, AgentKind.

use serde::{Deserialize, Serialize};

use crate::agent::classifier::TaskCategory;
use crate::types::{provider_for_cli, AgentKind, TaskBudget, TaskDifficulty};

pub(crate) const WEAKER_ON_CALLER_POOL: &str = "weaker model on caller's pool";

/// Why a candidate is not eligible: human text plus stable JSON codes.
#[derive(Debug, Default)]
pub(super) struct Exclusions {
    reasons: Vec<String>,
    codes: Vec<String>,
}

impl Exclusions {
    pub(super) fn push(&mut self, code: &str, reason: String) {
        self.codes.push(code.to_string());
        self.reasons.push(reason);
    }

    /// Floor check. A missing capability row defaults the base to 1; that is
    /// absent data, not a measured shortfall, so it gets its own reason.
    pub(super) fn floor(&mut self, base: Option<i32>, floor: i32, difficulty: TaskDifficulty, category: TaskCategory) {
        match base {
            None => self.push("no_capability_data", format!("no capability data for {}", category.label())),
            Some(base) if base < floor => self.push(
                "below_floor",
                format!("base {base} < floor {floor} for {}", difficulty.label()),
            ),
            Some(_) => {}
        }
    }

    pub(super) fn budget(&mut self, budget_ok: bool, budget: TaskBudget) {
        if !budget_ok {
            self.push("no_budget_model", format!("no model for budget {}", budget.label()));
        }
    }

    pub(super) fn is_empty(&self) -> bool {
        self.codes.is_empty()
    }

    pub(super) fn into_parts(self) -> (Option<String>, Vec<String>) {
        let reason = (!self.reasons.is_empty()).then(|| self.reasons.join("; "));
        (reason, self.codes)
    }
}

/// The session that asked for advice and the provider pool it draws on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub(crate) struct CallerAdvice {
    pub session: String,
    pub agent: String,
    pub provider: String,
    pub model: Option<String>,
    /// Catalog capability of `model`; `None` when the model is not known.
    pub capability: Option<f64>,
}

/// Resolve a caller session kind (`claude-code`, `codex`) to its pool.
pub(crate) fn caller_advice(session: &str, model: Option<&str>) -> Option<CallerAdvice> {
    let agent = crate::session::caller_agent(session)?;
    let (provider, _) = provider_for_cli(agent);
    let model = model.map(str::trim).filter(|value| !value.is_empty()).map(str::to_string);
    let capability = model.as_deref().and_then(|name| {
        crate::model_catalog::models_for_agent(&agent)
            .into_iter()
            .find(|item| item.model.eq_ignore_ascii_case(name))
            .and_then(|item| item.capability)
    });
    Some(CallerAdvice {
        session: session.to_string(),
        agent: agent.as_str().to_string(),
        provider: provider.as_str().to_string(),
        model,
        capability,
    })
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum PoolVerdict {
    /// Different pool, or same pool with a model at least as capable.
    Clear,
    /// Same pool, provably weaker than the caller's model: excluded.
    Weaker,
    /// Same pool, but the comparison cannot be made: ranked last, not excluded.
    Demote,
}

pub(super) fn pool_verdict(caller: Option<&CallerAdvice>, kind: AgentKind, capability: Option<f64>) -> PoolVerdict {
    let Some(caller) = caller else { return PoolVerdict::Clear };
    let (provider, _) = provider_for_cli(kind);
    if provider.is_unknown() || provider.as_str() != caller.provider {
        return PoolVerdict::Clear;
    }
    match (caller.capability, capability) {
        (Some(mine), Some(theirs)) if theirs < mine => PoolVerdict::Weaker,
        (Some(_), Some(_)) => PoolVerdict::Clear,
        _ => PoolVerdict::Demote,
    }
}

pub(super) fn demotion_reason(caller: &CallerAdvice) -> String {
    let model = caller.model.as_deref().unwrap_or("unknown");
    format!(
        "same pool as caller ({}); cannot compare with caller model {model}; ranked below other pools",
        caller.provider
    )
}

#[cfg(test)]
#[path = "selection_advice_gate_tests.rs"]
mod tests;
