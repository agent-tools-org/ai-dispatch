// Round-two regressions for toolbox bounds and count-cap omission reporting.
// Exercises the real prompt bundle builder and its single stored warning.
use super::*;

fn warning(store: &Store) -> String {
    let events: Vec<_> = store
        .get_events("t-round2")
        .unwrap()
        .into_iter()
        .filter(|event| event.detail.starts_with("Prompt context bounded:"))
        .collect();
    assert_eq!(events.len(), 1);
    events[0].detail.clone()
}

fn bundle(store: &Store, args: &RunArgs, root: &std::path::Path) -> PromptBundle {
    store
        .insert_waiting_task(
            "t-round2",
            "codex",
            &args.prompt,
            None,
            None,
            None,
            None,
            None,
            None,
            false,
            false,
        )
        .unwrap();
    build_prompt_bundle(
        store,
        args,
        &AgentKind::Codex,
        None,
        &[],
        "t-round2",
        None,
        Some(root),
    )
    .unwrap()
}

#[test]
fn round2_preservation_toolbox_bounds_large_description_and_many_tools() {
    for (count, length) in [(1, 100_000), (100, 1000)] {
        let temp = tempfile::tempdir().unwrap();
        let _home = crate::paths::AidHomeGuard::set(temp.path());
        let tools = toolbox::tools_dir();
        std::fs::create_dir_all(&tools).unwrap();
        for n in 0..count {
            std::fs::write(
                tools.join(format!("tool-{n}.toml")),
                format!(
                    "[tool]\nid = \"tool-{n}\"\ncommand = \"echo\"\ndescription = \"{}\"\n",
                    "é".repeat(length),
                ),
            )
            .unwrap();
        }
        let store = Store::open_memory().unwrap();
        let args = RunArgs {
            prompt: "Caller brief\n".repeat(1000),
            ..Default::default()
        };
        let prompt = bundle(&store, &args, temp.path()).effective_prompt;
        assert!(prompt.contains(&args.prompt));
        assert!(prompt.len() < args.prompt.len() + 15_000);
        assert!(prompt.contains("Toolbox: "));
        assert!(prompt.contains("characters omitted"));
        assert!(warning(&store).contains("Toolbox: "));
    }
}

#[test]
fn round2_preservation_knowledge_and_memory_caps_share_warning() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::paths::AidHomeGuard::set(temp.path());
    let team = crate::team::teams_dir().join("round2");
    std::fs::create_dir_all(&team).unwrap();
    let index = (0..6)
        .map(|n| format!("- [rust memory {n}] — rust memory reference\n"))
        .collect::<String>();
    std::fs::write(team.join("KNOWLEDGE.md"), index).unwrap();
    let store = Store::open_memory().unwrap();
    for n in 0..11 {
        store
            .insert_memory(&Memory {
                id: MemoryId(format!("m-{n}")),
                memory_type: MemoryType::Fact,
                tier: MemoryTier::Identity,
                content: format!("Identity {n}"),
                source_task_id: None,
                agent: None,
                project_path: Some(temp.path().to_str().unwrap().into()),
                content_hash: format!("hash-{n}"),
                created_at: chrono::Local::now(),
                expires_at: None,
                supersedes: None,
                version: 1,
                inject_count: 0,
                last_injected_at: None,
                success_count: 0,
            })
            .unwrap();
    }
    let args = RunArgs {
        prompt: "rust memory Caller brief".into(),
        team: Some("round2".into()),
        ..Default::default()
    };
    let prompt = bundle(&store, &args, temp.path()).effective_prompt;
    assert!(prompt.contains(&args.prompt));
    assert!(prompt.contains("Knowledge: 1 eligible entries omitted"));
    assert!(prompt.contains("Memory: 1 eligible entries omitted"));
    let event = warning(&store);
    assert!(event.contains("Knowledge: 1 eligible entries omitted"));
    assert!(event.contains("Memory: 1 eligible entries omitted"));
}
