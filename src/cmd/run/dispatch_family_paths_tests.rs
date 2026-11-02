// Family invariant through real dry-run, retry, cascade and batch entry points.
// Uses isolated state and served models; no provider command is executed.
use super::*;
use crate::types::{Task, TaskId, TaskStatus};

struct Fixture {
    home: crate::support::test_env::TestHome,
    _cache: crate::routing::quota::live::CacheDirGuard,
    _models: agent::model_validation::MockServedModelsGuard,
    store: Arc<Store>,
}

impl Fixture {
    fn new() -> Self {
        let home = crate::support::test_env::TestHome::with_dirs();
        let cache = crate::routing::quota::live::CacheDirGuard::set(home.path());
        let models = agent::model_validation::MockServedModelsGuard::set(
            AgentKind::Antigravity,
            Some(vec![
                "gemini-4-argon-medium".into(),
                "claude-opus-5-5-medium".into(),
            ]),
        );
        rate_limit::mark_untrusted_for_model(
            &AgentKind::Antigravity,
            None,
            Some("gemini-4-argon-medium"),
            "Individual quota reached. Resets in 36h29m20s.",
        );
        Self {
            home,
            _cache: cache,
            _models: models,
            store: Arc::new(Store::open_memory().expect("store")),
        }
    }

    fn args(&self) -> RunArgs {
        RunArgs {
            agent_name: "agy".into(),
            prompt: "say hi".into(),
            dry_run: true,
            dir: Some(self.home.path().display().to_string()),
            skills: vec![crate::cmd::run::NO_SKILL_SENTINEL.into()],
            ..Default::default()
        }
    }

    fn assert_child(&self, id: &TaskId) {
        let task = self
            .store
            .get_task(id.as_str())
            .expect("get")
            .expect("task");
        assert_eq!(task.agent, AgentKind::Antigravity);
        assert_eq!(
            task.requested_model.as_deref(),
            Some("claude-opus-5-5-medium")
        );
    }
}

#[tokio::test]
async fn agy_family_run_retry_and_cascade_check_the_same_boundary() {
    let f = Fixture::new();
    let id = crate::cmd::run::run(f.store.clone(), f.args())
        .await
        .expect("run");
    f.assert_child(&id);
    let mut parent = Task::test_fixture(TaskId("t-family-parent".into()), TaskStatus::Failed);
    parent.agent = AgentKind::Antigravity;
    parent.requested_model = Some("gemini-4-argon-medium".into());
    f.store.insert_task(&parent).expect("parent");
    f.store
        .update_task_dispatch_args(
            parent.id.as_str(),
            &f.args().dispatch_args_json().expect("args"),
        )
        .expect("saved args");
    let id = crate::cmd::retry::retry_task(
        f.store.clone(),
        crate::cmd::retry::RetryArgs {
            task_id: parent.id.to_string(),
            feedback: Some("try again".into()),
            ..Default::default()
        },
        false,
    )
    .await
    .expect("retry");
    f.assert_child(&id);
    let mut args = f.args();
    args.agent_name = "codex".into();
    args.cascade = vec!["agy".into()];
    let id = crate::cmd::run::run_lifecycle::retry_or_cascade(&f.store, &args, &parent.id)
        .await
        .expect("cascade")
        .expect("child");
    f.assert_child(&id);
}

#[tokio::test]
async fn agy_family_batch_preview_checks_the_same_boundary() {
    let f = Fixture::new();
    let file = f.home.path().join("batch.toml");
    std::fs::write(&file, format!(
        "[defaults]\ndir = {:?}\ndifficulty = 'moderate'\nbudget = 'standard'\nurgency = 'normal'\nrigor = 'standard'\n[[tasks]]\nname = 'family'\nagent = 'agy'\nprompt = 'say hi'\n",
        f.home.path().display().to_string())).expect("batch");
    crate::cmd::batch::run(
        f.store.clone(),
        crate::cmd::batch::BatchArgs {
            file: file.display().to_string(),
            vars: vec![],
            group: None,
            repo_root: None,
            parallel: false,
            analyze: false,
            wait: false,
            dry_run: true,
            no_prompt: true,
            yes: true,
            force: false,
            max_concurrent: None,
        },
    )
    .await
    .expect("batch preview");
    let tasks = f
        .store
        .list_tasks(crate::types::TaskFilter::All)
        .expect("tasks");
    assert_eq!(tasks.len(), 1);
    f.assert_child(&tasks[0].id);
}
