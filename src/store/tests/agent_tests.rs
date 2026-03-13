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
