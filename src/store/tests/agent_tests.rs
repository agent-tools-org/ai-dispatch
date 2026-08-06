// Agent success rate Store tests.
// Exports: agent_success_rates coverage.
// Deps: Store.

use super::*;

#[test]
fn agent_success_rates_returns_empty_for_no_tasks() {
    let store = Store::open_memory().unwrap();
    let rates = store.agent_success_rates().unwrap();
    assert!(rates.is_empty());
}

#[test]
fn agent_success_rates_filters_agents_with_fewer_than_five_tasks() {
    let store = Store::open_memory().unwrap();
    for i in 0..4 {
        let task = make_task(&format!("t-{:04}", i), AgentKind::Codex, TaskStatus::Done);
        store.insert_task(&task).unwrap();
    }
    let rates = store.agent_success_rates().unwrap();
    assert!(rates.is_empty());
}

#[test]
fn agent_success_rates_calculates_success_rate_correctly() {
    let store = Store::open_memory().unwrap();
    for i in 0..5 {
        let status = if i < 3 {
            TaskStatus::Done
        } else {
            TaskStatus::Failed
        };
        let task = make_task(&format!("t-{:04}", i), AgentKind::Codex, status);
        store.insert_task(&task).unwrap();
    }
    let rates = store.agent_success_rates().unwrap();
    assert_eq!(rates.len(), 1);
    let (agent, rate, count) = &rates[0];
    assert_eq!(*agent, AgentKind::Codex);
    assert_eq!(*count, 5);
    assert!((rate - 0.6).abs() < 0.01);
}

#[test]
fn agent_success_rates_includes_merged_as_success() {
    let store = Store::open_memory().unwrap();
    for i in 0..5 {
        let status = if i < 4 {
            TaskStatus::Merged
        } else {
            TaskStatus::Failed
        };
        let task = make_task(&format!("t-{:04}", i), AgentKind::Gemini, status);
        store.insert_task(&task).unwrap();
    }
    let rates = store.agent_success_rates().unwrap();
    let (agent, rate, count) = &rates[0];
    assert_eq!(*agent, AgentKind::Gemini);
    assert_eq!(*count, 5);
    assert!((rate - 0.8).abs() < 0.01);
}

#[test]
fn agent_success_rates_groups_by_agent() {
    let store = Store::open_memory().unwrap();
    for i in 0..5 {
        let task = make_task(&format!("t-c{:04}", i), AgentKind::Codex, TaskStatus::Done);
        store.insert_task(&task).unwrap();
    }
    for i in 0..5 {
        let status = if i < 2 {
            TaskStatus::Done
        } else {
            TaskStatus::Failed
        };
        let task = make_task(&format!("t-g{:04}", i), AgentKind::Gemini, status);
        store.insert_task(&task).unwrap();
    }
    let rates = store.agent_success_rates().unwrap();
    assert_eq!(rates.len(), 2);
    let codex_rate = rates
        .iter()
        .find(|(a, _, _)| *a == AgentKind::Codex)
        .unwrap();
    let gemini_rate = rates
        .iter()
        .find(|(a, _, _)| *a == AgentKind::Gemini)
        .unwrap();
    assert_eq!(codex_rate.2, 5);
    assert_eq!(gemini_rate.2, 5);
    assert!((codex_rate.1 - 1.0).abs() < 0.01);
    assert!((gemini_rate.1 - 0.4).abs() < 0.01);
}

#[test]
fn agent_success_rates_by_category_filters_correctly() {
    let store = Store::open_memory().unwrap();
    for i in 0..5 {
        let mut task = make_task(&format!("t-c{:04}", i), AgentKind::Codex, TaskStatus::Done);
        task.category = Some("debugging".to_string());
        store.insert_task(&task).unwrap();
    }
    for i in 0..5 {
        let mut task = make_task(&format!("t-g{:04}", i), AgentKind::Gemini, TaskStatus::Failed);
        task.category = Some("testing".to_string());
        store.insert_task(&task).unwrap();
    }

    let rates = store.agent_success_rates_by_category("debugging").unwrap();
    assert_eq!(rates.len(), 1);
    assert_eq!(rates[0].0, AgentKind::Codex);
    assert_eq!(rates[0].2, 5);
    assert!((rates[0].1 - 1.0).abs() < 0.01);
}

#[test]
fn agent_success_rates_by_category_empty_for_unknown() {
    let store = Store::open_memory().unwrap();
    for i in 0..5 {
        let mut task = make_task(&format!("t-c{:04}", i), AgentKind::Codex, TaskStatus::Done);
        task.category = Some("debugging".to_string());
        store.insert_task(&task).unwrap();
    }

    let rates = store.agent_success_rates_by_category("documentation").unwrap();
    assert!(rates.is_empty());
}

/// A database written by a newer aid — or read after a rollback — carries agent
/// names this binary has no variant for. Collapsing those to `custom` with the
/// name discarded left the board rendering `custom/unknown/unknown`, so the
/// reader could not tell which agent had actually run (t-8e9194dc, `commandcode`).
#[test]
fn unrecognised_agent_name_is_preserved_rather_than_becoming_a_nameless_custom() {
    let store = Store::open_memory().unwrap();
    let task = make_task("t-future", AgentKind::Codex, TaskStatus::Running);
    store.insert_task(&task).unwrap();
    store
        .conn
        .lock()
        .unwrap()
        .execute("UPDATE tasks SET agent = 'agent-from-the-future' WHERE id = 't-future'", [])
        .unwrap();

    let loaded = store.get_task("t-future").unwrap().unwrap();
    assert_eq!(loaded.agent, AgentKind::Custom);
    assert_eq!(
        loaded.custom_agent_name.as_deref(),
        Some("agent-from-the-future"),
        "the unparsed name must survive, not be replaced by a nameless custom"
    );
    // Closing the loop: the board and TUI read this, so the reader sees the real
    // name instead of the word "custom".
    assert_eq!(loaded.agent_display_name(), "agent-from-the-future");
}

/// A genuine custom agent already stores its own name; that must still win.
#[test]
fn a_real_custom_agent_keeps_its_configured_name() {
    let store = Store::open_memory().unwrap();
    let mut task = make_task("t-custom", AgentKind::Custom, TaskStatus::Done);
    task.custom_agent_name = Some("auditor".to_string());
    store.insert_task(&task).unwrap();

    let loaded = store.get_task("t-custom").unwrap().unwrap();
    assert_eq!(loaded.custom_agent_name.as_deref(), Some("auditor"));
}
