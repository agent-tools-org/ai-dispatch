// Regression tests for source-aware served-model caching.
// Deps: isolated Codex/AID homes and counting adapters; exports the test override guard.

use super::*;
use std::process::Command;

thread_local! {
    static TEST_OVERRIDE: std::cell::RefCell<HashMap<AgentKind, Option<Vec<String>>>> =
        std::cell::RefCell::new(HashMap::new());
    static TEST_CACHE: &'static Mutex<HashMap<AgentKind, CachedServedModels>> =
        Box::leak(Box::new(Mutex::new(HashMap::new())));
}

pub(super) fn cache() -> &'static Mutex<HashMap<AgentKind, CachedServedModels>> {
    TEST_CACHE.with(|cache| *cache)
}

pub(super) fn mock_models(kind: AgentKind) -> Option<Option<Vec<String>>> {
    TEST_OVERRIDE.with(|cell| cell.borrow().get(&kind).cloned())
}

pub(crate) struct MockServedModelsGuard;

impl MockServedModelsGuard {
    pub fn set(kind: AgentKind, models: Option<Vec<String>>) -> Self {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().insert(kind, models);
        });
        Self
    }
}

impl Drop for MockServedModelsGuard {
    fn drop(&mut self) {
        TEST_OVERRIDE.with(|cell| {
            cell.borrow_mut().clear();
        });
    }
}

use crate::agent::codex::{CodexAgent, cli_config::set_test_codex_home};
use crate::agent::RunOpts;
use crate::types::{TaskEvent, TaskId};
use std::sync::atomic::{AtomicUsize, Ordering};

struct CodexHomeGuard;
impl Drop for CodexHomeGuard {
    fn drop(&mut self) { set_test_codex_home(None); }
}

struct CountingCodex { probes: AtomicUsize }
impl CountingCodex {
    fn new() -> Self { Self { probes: AtomicUsize::new(0) } }
}
impl Agent for CountingCodex {
    fn kind(&self) -> AgentKind { AgentKind::Codex }
    fn streaming(&self) -> bool { false }
    fn accepts_interactive_input(&self) -> bool { false }
    fn build_command(&self, _: &str, _: &RunOpts) -> Result<Command> { Ok(Command::new("true")) }
    fn parse_event(&self, _: &TaskId, _: &str) -> Option<TaskEvent> { None }
    fn served_models_fingerprint(&self) -> Option<String> { CodexAgent.served_models_fingerprint() }
    fn served_models(&self) -> Result<Option<Vec<String>>> {
        self.probes.fetch_add(1, Ordering::SeqCst);
        CodexAgent.served_models()
    }
}

fn write_models(home: &std::path::Path, slug: &str) {
    std::fs::write(home.join("models_cache.json"),
        serde_json::json!({"models": [{"slug": slug}]}).to_string()).expect("models cache");
}

fn isolated_home(home: &std::path::Path) -> CodexHomeGuard {
    set_test_codex_home(Some(home.to_path_buf()));
    clear_served_models_cache();
    CodexHomeGuard
}

#[test]
fn codex_source_rewrite_invalidates_memory_inside_ttl_and_unchanged_source_is_cached() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    let _codex = isolated_home(temp.path());
    write_models(temp.path(), "gpt-6-sol");
    let agent = CountingCodex::new();
    assert_eq!(get_served_models_cached(&agent), Some(vec!["gpt-6-sol".into()]));
    assert_eq!(get_served_models_cached(&agent), Some(vec!["gpt-6-sol".into()]));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 1);
    let before = agent.served_models_fingerprint();
    write_models(temp.path(), "gpt-6.1-sol");
    assert_ne!(before, agent.served_models_fingerprint());
    assert_eq!(get_served_models_cached(&agent), Some(vec!["gpt-6.1-sol".into()]));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 2);
    assert_eq!(get_served_models_cached(&agent), Some(vec!["gpt-6.1-sol".into()]));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 2);
    let entry = load_disk_entry(AgentKind::Codex, agent.served_models_fingerprint().as_deref())
        .expect("fresh entry within TTL");
    assert_eq!(entry.fingerprint, agent.served_models_fingerprint());
}

#[test]
fn codex_source_rewrite_invalidates_disk_inside_ttl_and_unchanged_disk_is_cached() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    let _codex = isolated_home(temp.path());
    write_models(temp.path(), "gpt-6-sol");
    let agent = CountingCodex::new();
    get_served_models_cached(&agent).expect("initial models");
    cache().lock().expect("cache lock").clear();
    assert_eq!(get_served_models_cached_with_status(&agent), (Some(vec!["gpt-6-sol".into()]), false));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 1);
    cache().lock().expect("cache lock").clear();
    write_models(temp.path(), "gpt-6.1-sol");
    assert_eq!(get_served_models_cached_with_status(&agent), (Some(vec!["gpt-6.1-sol".into()]), true));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 2);
}

#[test]
fn legacy_codex_entry_without_fingerprint_refreshes_from_source() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    let _codex = isolated_home(temp.path());
    write_models(temp.path(), "gpt-6.1-sol");
    let legacy = serde_json::json!({"codex": {"models": ["gpt-6-sol"], "updated_at_secs": now_secs()}});
    let entries: HashMap<String, ServedModelsCacheEntry> = serde_json::from_value(legacy.clone()).expect("legacy parses");
    assert_eq!(entries["codex"].fingerprint, None);
    std::fs::write(cache_file_path(), legacy.to_string()).expect("legacy disk cache");
    assert_eq!(get_served_models_cached(&CodexAgent), Some(vec!["gpt-6.1-sol".into()]));
}

#[test]
fn codex_catalog_lookup_refreshes_after_source_rewrite() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    let _codex = isolated_home(temp.path());
    write_models(temp.path(), "gpt-6-sol");
    assert_eq!(load_from_disk_cache(AgentKind::Codex), Some(vec!["gpt-6-sol".into()]));
    write_models(temp.path(), "gpt-99.7-source-model");
    let store = crate::store::Store::open_memory().expect("store");
    let json = crate::cmd::agent_json::agents_list_value(&store).expect("agent list JSON");
    let codex = json["agents"].as_array().expect("agents").iter()
        .find(|agent| agent["name"] == "codex").expect("codex");
    assert!(codex["models"]["available"].as_array().expect("models").iter()
        .any(|row| row["model"] == "gpt-99.7-source-model" && row["source"] == "served"));
    assert_eq!(load_from_disk_cache(AgentKind::Codex), Some(vec!["gpt-99.7-source-model".into()]));
}

#[test]
fn codex_fingerprint_is_none_for_missing_source_and_none_agents_keep_cache() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    let _codex = isolated_home(temp.path());
    assert_eq!(CodexAgent.served_models_fingerprint(), None);
    let agent = CountingCodex::new();
    save_to_disk_cache(AgentKind::Codex, &["cached-model".into()], Some("old-stamp".into()));
    assert_eq!(get_served_models_cached(&agent), Some(vec!["cached-model".into()]));
    assert_eq!(agent.probes.load(Ordering::SeqCst), 0);
    assert_eq!(crate::agent::grok::GrokAgent.served_models_fingerprint(), None);
}

#[test]
fn custom_agent_disk_lookup_remains_cache_only() {
    let temp = tempfile::tempdir().expect("tempdir");
    let _aid = crate::paths::AidHomeGuard::set(temp.path());
    assert_eq!(load_from_disk_cache(AgentKind::Custom), None);
    save_to_disk_cache(AgentKind::Custom, &["custom-model".into()], None);
    assert_eq!(load_from_disk_cache(AgentKind::Custom), Some(vec!["custom-model".into()]));
}
