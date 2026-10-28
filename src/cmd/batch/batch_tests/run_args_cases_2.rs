// Batch regression cases relocated to keep test modules bounded.
// Deps: shared fixtures and imports from the parent test module.
use super::*;

#[test]
fn task_to_run_args_defaults_dry_run_to_false() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let run_args = task_to_run_args(
        &batch::BatchTask {
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            ..Default::default()
        },
        &[],
        false,
        &store,
        None,
    );

    assert!(!run_args.dry_run);
}

#[test]
fn task_to_run_args_includes_sibling_metadata() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let current = batch::BatchTask {
        id: Some("task-current".to_string()),
        name: Some("current".to_string()),
        agent: "codex".to_string(),
        prompt: "implement the feature".to_string(),
        ..Default::default()
    };
    let sibling = batch::BatchTask {
        id: Some("task-sibling".to_string()),
        name: Some("sibling".to_string()),
        agent: "grok".to_string(),
        prompt: "review the implementation".to_string(),
        ..Default::default()
    };

    let run_args = task_to_run_args(&current, &[&sibling], true, &store, None);

    assert_eq!(
        run_args.batch_siblings,
        vec![(
            "sibling".to_string(),
            "grok".to_string(),
            "review the implementation".to_string(),
        )]
    );
}

#[test]
fn task_to_run_args_applies_forwarded_env_after_explicit_env() {
    let _guard = isolated_home();
    let store = Arc::new(Store::open_memory().unwrap());
    let forwarded_path = std::env::var("PATH").unwrap();
    let run_args = task_to_run_args(
        &batch::BatchTask {
            agent: "codex".to_string(),
            prompt: "test".to_string(),
            env: Some([("PATH".to_string(), "explicit".to_string())].into_iter().collect()),
            env_forward: Some(vec!["PATH".to_string()]),
            ..Default::default()
        },
        &[],
        true,
        &store,
        None,
    );

    // The forwarded value is resolved at launch (apply_run_env applies
    // env_forward after env), so run args keep only the name.
    assert_eq!(
        run_args.env.as_ref().and_then(|env| env.get("PATH")).map(String::as_str),
        Some("explicit")
    );
    assert_ne!(forwarded_path, "explicit");
    assert_eq!(run_args.env_forward, Some(vec!["PATH".to_string()]));
}
