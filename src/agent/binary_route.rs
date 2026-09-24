// Route eligibility on this host: the single predicate advise, agent list and run share.
// Exports: RouteBlocker, route_blocker, custom_route_blocker, route_inventory, detect_agents.
// Deps: AgentKind, built_in_program, program_blocker, env::which_exists.

use super::{built_in_program, program_blocker};
use crate::agent::env;
use crate::types::AgentKind;

/// Why a built-in route cannot run on this host.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum RouteBlocker {
    NotInstalled { binary: String },
}

impl RouteBlocker {
    /// Stable JSON reason code.
    pub(crate) fn code(&self) -> &'static str {
        match self {
            Self::NotInstalled { .. } => "not_installed",
        }
    }

    /// Human reason used by advise and the run preflight error.
    pub(crate) fn detail(&self) -> String {
        match self {
            Self::NotInstalled { binary } => format!("binary '{binary}' missing from PATH"),
        }
    }

    pub(crate) fn reason(&self) -> String {
        match self {
            Self::NotInstalled { .. } => format!("not installed: {}", self.detail()),
        }
    }
}

/// Every built-in route aid lists and advises on. `claude` is kept out of
/// `ALL_BUILTIN` (auto-selection and fallback chains skip it) but is still a
/// route an operator can dispatch, so listings and advice must show it.
pub(crate) fn routable_builtins() -> impl Iterator<Item = AgentKind> {
    AgentKind::ALL_BUILTIN.iter().copied().chain([AgentKind::Claude])
}

/// `None` when the route can run on this host.
pub(crate) fn route_blocker(kind: AgentKind) -> Option<RouteBlocker> {
    route_blocker_with(kind, env::which_exists)
}

pub(crate) fn route_blocker_with<F>(kind: AgentKind, which: F) -> Option<RouteBlocker>
where
    F: Fn(&str) -> bool,
{
    program_blocker(built_in_program(kind)?, &which)
}

/// The same predicate for a custom agent: the first word of its command.
pub(crate) fn custom_route_blocker(command: &str) -> Option<RouteBlocker> {
    custom_route_blocker_with(command, env::which_exists)
}

pub(crate) fn custom_route_blocker_with<F>(command: &str, which: F) -> Option<RouteBlocker>
where
    F: Fn(&str) -> bool,
{
    program_blocker(command.split_whitespace().next().unwrap_or_default(), &which)
}

/// The predicate evaluated for every routable built-in, probed in parallel.
pub(crate) fn route_inventory() -> Vec<(AgentKind, Option<RouteBlocker>)> {
    #[cfg(test)]
    if let Some(list) = DETECT_AGENTS_OVERRIDE.with(|cell| cell.borrow().clone()) {
        // Pinned inventory never probes the host, not even Cursor's `agent` identity.
        let _cursor = crate::agent::cursor::CursorBinaryGuard::set("cursor-agent");
        return routable_builtins()
            .map(|kind| {
                let blocker = if list.contains(&kind) {
                    None
                } else {
                    route_blocker_with(kind, |_| false)
                };
                (kind, blocker)
            })
            .collect();
    }
    let kinds: Vec<AgentKind> = routable_builtins().collect();
    std::thread::scope(|scope| {
        kinds
            .iter()
            .map(|kind| scope.spawn(move || (*kind, route_blocker(*kind))))
            .collect::<Vec<_>>()
            .into_iter()
            .filter_map(|probe| probe.join().ok())
            .collect()
    })
}

/// Installed built-in routes: `route_inventory` entries with no blocker.
pub fn detect_agents() -> Vec<AgentKind> {
    route_inventory()
        .into_iter()
        .filter_map(|(kind, blocker)| blocker.is_none().then_some(kind))
        .collect()
}

#[cfg(test)]
std::thread_local! {
    static DETECT_AGENTS_OVERRIDE: std::cell::RefCell<Option<Vec<AgentKind>>> =
        const { std::cell::RefCell::new(None) };
}

/// RAII guard that pins the route inventory to a test-supplied installed list
/// on the current thread. Restores the previous value on drop.
#[cfg(test)]
pub(crate) struct DetectAgentsGuard {
    previous: Option<Vec<AgentKind>>,
}

#[cfg(test)]
impl DetectAgentsGuard {
    pub fn set(agents: Vec<AgentKind>) -> Self {
        let previous = DETECT_AGENTS_OVERRIDE.with(|cell| cell.borrow().clone());
        DETECT_AGENTS_OVERRIDE.with(|cell| *cell.borrow_mut() = Some(agents));
        Self { previous }
    }
}

#[cfg(test)]
impl Drop for DetectAgentsGuard {
    fn drop(&mut self) {
        DETECT_AGENTS_OVERRIDE.with(|cell| *cell.borrow_mut() = self.previous.take());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn missing_binary_blocks_route_with_reason() {
        let blocker = route_blocker_with(AgentKind::Antigravity, |_| false).expect("blocked");
        assert_eq!(blocker.code(), "not_installed");
        assert_eq!(blocker.reason(), "not installed: binary 'agy' missing from PATH");
    }

    #[test]
    fn present_binary_has_no_blocker() {
        assert_eq!(route_blocker_with(AgentKind::Grok, |name| name == "grok"), None);
    }

    #[test]
    fn custom_command_uses_its_first_word() {
        assert_eq!(custom_route_blocker_with("goose run", |name| name == "goose"), None);
        let blocker = custom_route_blocker_with("goose run", |_| false).expect("blocked");
        assert_eq!(blocker.reason(), "not installed: binary 'goose' missing from PATH");
        assert!(custom_route_blocker_with("", |_| true).is_some());
    }

    #[test]
    fn inventory_lists_claude_and_blocks_uninstalled() {
        let _guard = DetectAgentsGuard::set(vec![AgentKind::Codex]);
        let inventory = route_inventory();
        assert!(inventory.iter().any(|(kind, _)| *kind == AgentKind::Claude));
        let blocked = |target| {
            inventory.iter().find(|(kind, _)| *kind == target).and_then(|(_, b)| b.clone())
        };
        assert_eq!(blocked(AgentKind::Codex), None);
        assert!(blocked(AgentKind::Antigravity).is_some());
        assert_eq!(detect_agents(), vec![AgentKind::Codex]);
    }
}
