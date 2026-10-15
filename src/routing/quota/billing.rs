// Billing evidence from aidbar and declared-profile selection policy.
// Credit balances retain provider units; they are never interpreted as dollars.

use crate::types::{DeclaredTaskProfile, TaskBudget, TaskRigor};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum BillingState {
    InPlan,
    Overage,
    Exhausted,
    #[serde(other)]
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Credits {
    pub balance: f64,
    pub unlimited: bool,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub(crate) struct Billing {
    pub state: BillingState,
    pub credits: Option<Credits>,
}

impl Billing {
    pub(crate) fn label(&self) -> String {
        let state = match self.state {
            BillingState::InPlan => "in_plan",
            BillingState::Overage => "overage",
            BillingState::Exhausted => "exhausted",
            BillingState::Unknown => "unknown",
        };
        match &self.credits {
            Some(credits) => format!("{state} (credits balance {})", credits.balance),
            None => state.to_string(),
        }
    }

    pub(crate) fn exclusion(&self, profile: DeclaredTaskProfile) -> Option<String> {
        match self.state {
            BillingState::Exhausted => Some(self.label()),
            BillingState::Overage if matches!(profile.budget, TaskBudget::Free | TaskBudget::Cheap) => {
                Some(format!("{}; budget {}", self.label(), profile.budget.label()))
            }
            _ => None,
        }
    }

    pub(crate) fn penalty(&self, profile: DeclaredTaskProfile) -> f64 {
        if self.state == BillingState::Overage
            && profile.budget == TaskBudget::Standard
            && profile.rigor != TaskRigor::Critical
        {
            -4.0
        } else {
            0.0
        }
    }
}

#[cfg(test)]
#[path = "billing_tests.rs"]
mod tests;
