// Pre-dispatch model validation and CLI served model probing.
// Exports: validate_model_for_agent, get_served_models_cached, clear_served_models_cache.
// Deps: Agent, AgentKind.

use anyhow::{Result, anyhow};
use std::collections::HashMap;
use std::process::Command;
use std::sync::{Mutex, OnceLock};
use std::time::Duration;
use serde::{Deserialize, Serialize};

use crate::types::AgentKind;
use super::Agent;

static SERVED_CACHE: OnceLock<Mutex<HashMap<AgentKind, Option<Vec<String>>>>> = OnceLock::new();

#[cfg(test)]
thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<HashMap<AgentKind, Option<Vec<String>>>> =
        std::cell::RefCell::new(HashMap::new());
}

fn cache() -> &'static Mutex<HashMap<AgentKind, Option<Vec<String>>>> {
    SERVED_CACHE.get_or_init(|| Mutex::new(HashMap::new()))
}

pub(crate) fn clear_served_models_cache() {
    if let Ok(mut guard) = cache().lock() {
        guard.clear();
    }
}

#[cfg(test)]
pub(crate) struct MockServedModelsGuard;

#[cfg(test)]
impl MockServedModelsGuard {
    pub fn set(kind: AgentKind, models: Option<Vec<String>>) -> Self {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().insert(kind, models);
        });
        Self
    }
}

#[cfg(test)]
impl Drop for MockServedModelsGuard {
    fn drop(&mut self) {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().clear();
        });
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub(crate) enum ModelSource {
    UserSupplied,
    AidResolved,
}

impl Default for ModelSource {
    fn default() -> Self {
        Self::UserSupplied
    }
}

pub(crate) fn validate_model_for_agent(
    agent: &dyn Agent,
    model: &str,
    source: ModelSource,
) -> Result<bool> {
    let model_clean = model.trim();
    if model_clean.is_empty() {
        return Ok(true);
    }
    let kind = agent.kind();
    let served = get_served_models_cached(agent);

    let Some(served_list) = served else {
        aid_info!(
            "[aid] Cannot query served models for {}; allowing dispatch with model '{model_clean}'",
            kind.as_str()
        );
        return Ok(true);
    };

    if served_list.is_empty() {
        aid_info!(
            "[aid] No served models reported for {}; allowing dispatch with model '{model_clean}'",
            kind.as_str()
        );
        return Ok(true);
    }

    if served_list.iter().any(|m| m.eq_ignore_ascii_case(model_clean)) {
        return Ok(true);
    }

    let list_str = served_list.join(", ");
    if source == ModelSource::UserSupplied {
        return Err(anyhow!(
            "Agent '{}' does not serve model '{model_clean}'. Served models: {list_str}",
            kind.as_str()
        ));
    }
    aid_warn!(
        "[aid] Agent '{}' does not serve aid-selected model '{model_clean}'; dropping it and using the agent's own default model",
        kind.as_str()
    );
    Ok(false)
}

pub(crate) fn get_served_models_cached(agent: &dyn Agent) -> Option<Vec<String>> {
    let kind = agent.kind();
    #[cfg(test)]
    {
        let thread_mock = TEST_OVERRIDE.with(|cell| cell.borrow().get(&kind).cloned());
        if let Some(res) = thread_mock {
            return res;
        }
        agent.served_models().ok().flatten()
    }
    #[cfg(not(test))]
    {
        if let Ok(guard) = cache().lock() {
            if let Some(cached) = guard.get(&kind) {
                return cached.clone();
            }
        }
        let result = agent.served_models().ok().flatten();
        if let Ok(mut guard) = cache().lock() {
            guard.insert(kind, result.clone());
        }
        result
    }
}

pub(crate) fn run_cmd_with_timeout(mut cmd: Command, timeout: Duration) -> Option<String> {
    let (tx, rx) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let res = cmd.output().ok().and_then(|output| {
            if output.status.success() {
                let stdout = String::from_utf8_lossy(&output.stdout).to_string();
                let stderr = String::from_utf8_lossy(&output.stderr).to_string();
                Some(format!("{stdout}\n{stderr}"))
            } else {
                None
            }
        });
        let _ = tx.send(res);
    });
    rx.recv_timeout(timeout).ok().flatten()
}

#[cfg(test)]
#[path = "model_validation_tests.rs"]
mod tests;
