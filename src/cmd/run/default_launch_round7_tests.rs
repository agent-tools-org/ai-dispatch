// Default retry controls exercise real dispatch and local fake agents in the test binary.
// Caller-owned dirty preservation uses the same full worker/lifecycle path without a rescue agent.
use super::*;

const SUCCESS: &str = "printf '%s\\n' '{\"type\":\"completion\",\"finalText\":\"done\"}'";

fn install(home: &Path, name: &str, body: &str) {
    std::fs::create_dir_all(home.join("agents")).expect("agents");
    let fixed = serde_json::to_string(&vec!["-c", body]).expect("fixed args");
    std::fs::write(
        home.join(format!("agents/{name}.toml")),
        format!(
            "[agent]\nid='{name}'\ndisplay_name='{name}'\ncommand='/bin/sh'\n\
         prompt_mode='arg'\nstreaming=true\ninteractive_input=false\nfixed_args={fixed}\n"
        ),
    )
    .expect("agent config");
}

fn args(repo: &Path) -> RunArgs {
    RunArgs {
        agent_name: "round7-agent".into(),
        prompt: "complete the requested work".into(),
        repo: Some(repo.display().to_string()),
        dir: Some(repo.display().to_string()),
        skills: vec![NO_SKILL_SENTINEL.into()],
        no_audit: true,
        announce: false,
        ..Default::default()
    }
}

#[tokio::test]
async fn default_checklist_retry_still_creates_and_launches_its_child() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let repo = crate::support::test_repo::repo_with_file("tracked.txt", "base\n", "base");
    install(home.path(), "round7-agent", SUCCESS);
    let mut args = args(repo.path());
    args.retry = 1;
    args.checklist = vec!["report CHECKLIST 1: confirmed".into()];
    let id = run(store.clone(), args).await.expect("default retry");
    let root = store.find_retry_root(id.as_str()).expect("root").expect("task");
    let tree = store.get_retry_tree(root.id.as_str()).expect("retry tree");
    assert_eq!(tree.len(), 2);
    let child = tree
        .iter()
        .find(|task| task.parent_task_id.is_some())
        .expect("child");
    assert!(
        store
            .get_events(child.id.as_str())
            .expect("events")
            .iter()
            .any(|event| event.event_kind == EventKind::Completion)
    );
}

#[tokio::test]
async fn default_capacity_cascade_still_creates_and_launches_its_child() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let repo = crate::support::test_repo::repo_with_file("tracked.txt", "base\n", "base");
    install(
        home.path(),
        "round7-agent",
        concat!(
            "printf '%s\\n' '{\"type\":\"error\",\"message\":",
            "\"Selected model is at capacity. Please try a different model.\"}'; exit 1"
        ),
    );
    install(home.path(), "round7-peer", SUCCESS);
    let mut args = args(repo.path());
    args.cascade = vec!["round7-peer".into()];
    let id = run(store.clone(), args).await.expect("default cascade");
    let root = store.find_retry_root(id.as_str()).expect("root").expect("task");
    let tree = store.get_retry_tree(root.id.as_str()).expect("retry tree");
    assert_eq!(tree.len(), 2);
    let child = tree
        .iter()
        .find(|task| task.parent_task_id.is_some())
        .expect("child");
    assert_eq!(child.custom_agent_name.as_deref(), Some("round7-peer"));
    assert_eq!(child.status, TaskStatus::Done);
}

#[tokio::test]
async fn caller_owned_full_dispatch_commits_dirty_worktree_without_an_agent_retry() {
    let _permit = crate::support::test_subprocess::acquire();
    let (home, store) = crate::support::test_env::TestHome::with_dirs().shared_store();
    let repo = crate::support::test_repo::repo_with_file("tracked.txt", "base\n", "base");
    install(
        home.path(),
        "round7-agent",
        &format!("printf 'changed\\n' > tracked.txt; {SUCCESS}"),
    );
    let mut args = args(repo.path());
    args.policy = Some("caller-owned".into());
    args.existing_task_id = Some(TaskId("t-owned-preserve".into()));
    args.model = Some("exact-model".into());
    args.worktree = Some("test/round7-preserve".into());
    let id = run(store.clone(), args).await.expect("caller-owned run");
    let task = store.get_task(id.as_str()).expect("task").expect("row");
    assert_eq!(task.status, TaskStatus::Done);
    assert_eq!(store.list_tasks(TaskFilter::All).expect("rows").len(), 1);
    let wt = task.worktree_path.expect("worktree");
    assert!(!crate::workspace::commit::has_uncommitted_changes(&wt).expect("clean"));
    assert!(
        store
            .get_events(id.as_str())
            .expect("events")
            .iter()
            .any(|event| event.detail.contains("Rescued 1 file(s)"))
    );
}
