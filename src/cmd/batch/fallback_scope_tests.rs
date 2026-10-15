// Batch fallback authorization through the coordinator and real shell workers.
// Covers sibling isolation, explicit defaults, task-local lists and saved identity.
// Deps: isolated routing guards, batch command, Store and shell-backed agents.
use super::super::batch_dispatch_support::maybe_dispatch_auto_fallback;
use super::super::{BatchArgs, batch_types::BatchTaskOutcome};
use super::shared::make_stored_task;
use crate::agent::model_validation::MockServedModelsGuard;
use crate::agent::{DetectAgentsGuard, cursor::CursorBinaryGuard};
use crate::cmd::run::{RunArgs, advice_route};
use crate::{support::paths, store::Store, types::*};
use std::{fs, os::unix::fs::PermissionsExt, path::Path, sync::Arc};

fn install_agents(home: &Path) -> &'static str {
    let peer = home.join("peer.sh");
    fs::write(
        &peer,
        format!(
            "#!/bin/sh\ncase \"$1\" in models|--help) exit 0;; esac\n\
         printf '%s\\n' \"$AID_TASK_ID\" >> '{}'\n\
         printf '%s\\n' '{{\"type\":\"result\",\"is_error\":false,\"result\":\"fixture text\"}}'\n",
            home.join("peer-launches").display(),
        ),
    )
    .expect("peer script");
    fs::set_permissions(&peer, fs::Permissions::from_mode(0o755)).expect("executable");
    fs::create_dir_all(home.join("agents")).expect("agents");
    fs::create_dir_all(home.join("sibling")).expect("sibling directory");
    fs::write(
        home.join("agents/scope-source.toml"),
        "[agent]\nid = 'scope-source'\ndisplay_name = 'Source'\ncommand = '/bin/sh'\n\
         prompt_mode = 'arg'\nfixed_args = ['-c', 'exit 1']\ninteractive_input = false\n",
    )
    .expect("source agent");
    Box::leak(peer.display().to_string().into_boxed_str())
}

fn batch_contents(home: &Path, default: &str) -> String {
    format!(
        "[defaults]\n{default}\nagent = 'scope-source'\ndir = '{}'\nverify = 'true'\n\
         no_skill = true\ndifficulty = 'simple'\nbudget = 'standard'\n\
         urgency = 'normal'\nrigor = 'standard'\n\
         [[task]]\nid = 't-scope-a'\nname = 'A'\nprompt = 'Reply with fixture A'\n\
         model = 'source-model'\nretry = 0\n\
         [[task]]\nid = 't-scope-b'\nname = 'B'\nprompt = 'Reply with fixture B'\n\
         dir = '{}/sibling'\nfallback = 'cursor'\n",
        home.display(),
        home.display(),
    )
}

async fn run_batch(store: Arc<Store>, file: &Path, parallel: bool) {
    let result = tokio::time::timeout(
        std::time::Duration::from_secs(20),
        super::super::run(
            store,
            BatchArgs {
                file: file.display().to_string(),
                vars: vec![],
                group: None,
                repo_root: None,
                parallel,
                analyze: false,
                wait: true,
                dry_run: false,
                no_prompt: true,
                yes: true,
                force: false,
                max_concurrent: Some(2),
            },
        ),
    )
    .await
    .expect("batch timeout");
    if let Err(error) = result {
        assert!(error.to_string().contains("did not succeed"), "{error:#}");
    }
}

async fn batch_scope(default: &str, authorized: bool, parallel: bool) {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let _peer = CursorBinaryGuard::set(install_agents(home.path()));
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Cursor]);
    let _models = MockServedModelsGuard::set(AgentKind::Cursor, None);
    let store = Arc::new(Store::open_memory().expect("store"));
    let file = home.path().join("scope.toml");
    fs::write(&file, batch_contents(home.path(), default)).expect("batch file");
    run_batch(store.clone(), &file, parallel).await;
    let tasks = store.list_tasks(TaskFilter::All).expect("tasks");
    let a = store.get_task("t-scope-a").expect("lookup").expect("A");
    let saved = RunArgs::saved_for_task(&store, "t-scope-a")
        .expect("args")
        .expect("saved");
    assert_eq!(a.agent_display_name(), "scope-source");
    assert_eq!(a.requested_model.as_deref(), Some("source-model"));
    assert_eq!(saved.agent_name, "scope-source");
    assert_eq!(saved.model.as_deref(), Some("source-model"));
    assert!(saved.session_id.is_none());
    assert_eq!(
        tasks
            .iter()
            .filter(|t| t.parent_task_id.as_deref() == Some("t-scope-a"))
            .count(),
        usize::from(authorized),
        "A child authorization"
    );
    assert!(
        tasks
            .iter()
            .any(|t| t.parent_task_id.as_deref() == Some("t-scope-b")
                && t.agent == AgentKind::Cursor),
        "B must retain its fallback"
    );
    assert_peer_launches(home.path(), &tasks);
}

fn assert_peer_launches(home: &Path, tasks: &[Task]) {
    let launches = fs::read_to_string(home.join("peer-launches")).expect("peer launched for B");
    let peers: Vec<_> = tasks
        .iter()
        .filter(|task| task.agent == AgentKind::Cursor)
        .collect();
    assert_eq!(launches.lines().count(), peers.len(), "{launches}");
    assert!(
        peers
            .iter()
            .all(|task| launches.lines().any(|id| id == task.id.as_str())),
        "{launches}"
    );
}

#[tokio::test]
async fn sibling_fallback_does_not_authorize_failed_task_with_false_default() {
    batch_scope("auto_fallback = false", false, true).await;
}

#[tokio::test]
async fn sibling_fallback_does_not_authorize_failed_task_with_omitted_default() {
    batch_scope("", false, false).await;
}

#[tokio::test]
async fn explicit_batch_auto_fallback_authorizes_failed_task_without_fallback() {
    batch_scope("auto_fallback = true", true, true).await;
}

fn save_failed_task(store: &Store, home: &Path, id: &str, cascade: Vec<String>) {
    let mut task = make_stored_task(id, AgentKind::Codex, TaskStatus::Failed);
    task.requested_model = Some("gpt-6-sol".into());
    task.agent_session_id = Some("source-session".into());
    store.insert_task(&task).expect("task");
    let args = RunArgs {
        agent_name: "codex".into(),
        prompt: task.prompt.clone(),
        model: task.requested_model,
        session_id: task.agent_session_id,
        dir: Some(home.display().to_string()),
        verify: Some("true".into()),
        skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()],
        cascade,
        ..Default::default()
    };
    store
        .update_task_dispatch_args(id, &args.dispatch_args_json().expect("serialize"))
        .expect("save");
}

#[tokio::test]
async fn unauthorized_failed_task_keeps_saved_agent_model_session_and_has_no_peer_launch() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let _peer = CursorBinaryGuard::set(install_agents(home.path()));
    let _fleet = DetectAgentsGuard::set(vec![AgentKind::Cursor]);
    let _models = MockServedModelsGuard::set(AgentKind::Cursor, None);
    let store = Arc::new(Store::open_memory().expect("store"));
    save_failed_task(&store, home.path(), "t-saved-a", vec![]);
    let config: crate::batch::BatchConfig =
        toml::from_str(&batch_contents(home.path(), "auto_fallback = false")).expect("config");
    let before = RunArgs::saved_for_task(&store, "t-saved-a")
        .expect("args")
        .expect("saved");
    assert!(
        advice_route::automatic_candidate(Some(&store), &before).is_some(),
        "launchable peer"
    );
    let mut retried = vec![false; 2];
    let child = maybe_dispatch_auto_fallback(
        store.clone(),
        &config.tasks,
        0,
        "t-saved-a",
        BatchTaskOutcome::Failed,
        false,
        &mut retried,
        None,
        None,
    )
    .await
    .expect("fallback");
    assert!(child.is_none() && !retried[0]);
    assert_eq!(store.list_tasks(TaskFilter::All).expect("tasks").len(), 1);
    assert!(!home.path().join("peer-launches").exists());
    let after = RunArgs::for_retry(
        &store,
        &store.get_task("t-saved-a").expect("lookup").expect("A"),
    )
    .expect("retry args");
    assert_eq!(after.agent_name, before.agent_name);
    assert_eq!(after.model, before.model);
    assert_eq!(after.session_id, before.session_id);
}

#[tokio::test]
async fn task_local_cascade_authorizes_coordinator_fallback_with_false_default() {
    let home = tempfile::tempdir().expect("home");
    let _home = paths::AidHomeGuard::set(home.path());
    let _cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
    paths::ensure_dirs().expect("directories");
    let _peer = CursorBinaryGuard::set(install_agents(home.path()));
    let _models = MockServedModelsGuard::set(AgentKind::Cursor, None);
    let store = Arc::new(Store::open_memory().expect("store"));
    save_failed_task(&store, home.path(), "t-saved-b", vec!["cursor".into()]);
    let spec =
        toml::from_str("agent = 'codex'\nprompt = 'fixture'\nfallback = 'cursor'").expect("spec");
    let mut retried = vec![false];
    let child = maybe_dispatch_auto_fallback(
        store.clone(),
        &[spec],
        0,
        "t-saved-b",
        BatchTaskOutcome::Failed,
        false,
        &mut retried,
        None,
        None,
    )
    .await
    .expect("fallback");
    let child = child.expect("task-local fallback child");
    crate::cmd::wait::wait_for_task_ids(
        &store,
        &[child.clone()],
        None,
        false,
        Some(std::time::Duration::from_secs(10)),
    )
    .await
    .expect("wait");
    let task = store.get_task(&child).expect("lookup").expect("child");
    assert_eq!(task.parent_task_id.as_deref(), Some("t-saved-b"));
    assert_eq!(task.agent, AgentKind::Cursor);
    assert!(retried[0] && home.path().join("peer-launches").exists());
}
