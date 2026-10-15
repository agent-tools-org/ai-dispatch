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
        Some(&project::ProjectConfig {
            id: "round2".into(),
            ..Default::default()
        }),
        Some(root),
    )
    .unwrap()
}

#[test]
fn round2_preservation_toolbox_bounds_large_description_and_many_tools() {
    for (count, length) in [(1, 100_000), (100, 1000)] {
        let temp = tempfile::tempdir().unwrap();
        let _home = crate::support::paths::AidHomeGuard::set(temp.path());
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
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    let team = crate::team::teams_dir().join("round2");
    std::fs::create_dir_all(&team).unwrap();
    let index = (0..6)
        .map(|n| format!("- [rust memory {n}] — rust memory reference\n"))
        .collect::<String>();
    std::fs::write(team.join("KNOWLEDGE.md"), index).unwrap();
    let store = Store::open_memory().unwrap();
    for n in 0..11 {
        insert_memory(&store, temp.path(), n, MemoryTier::Identity, "Identity");
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

fn insert_memory(store: &Store, root: &std::path::Path, n: usize, tier: MemoryTier, text: &str) {
    store
        .insert_memory(&Memory {
            id: MemoryId(format!("m-{n}")),
            memory_type: MemoryType::Fact,
            tier,
            content: format!("{text} {n}"),
            source_task_id: None,
            agent: None,
            project_path: Some(root.to_str().unwrap().into()),
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

#[test]
fn round2_preservation_remaining_memory_slots_report_omissions() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    let store = Store::open_memory().unwrap();
    for n in 0..10 {
        insert_memory(&store, temp.path(), n, MemoryTier::Identity, "Identity");
    }
    for n in 10..12 {
        insert_memory(
            &store,
            temp.path(),
            n,
            MemoryTier::OnDemand,
            "rust memory Caller brief",
        );
    }
    let args = RunArgs {
        prompt: "rust memory Caller brief".into(),
        ..Default::default()
    };
    let prompt = bundle(&store, &args, temp.path()).effective_prompt;
    assert!(prompt.contains(&args.prompt));
    assert!(prompt.contains("Memory: 2 eligible entries omitted"));
    assert!(warning(&store).contains("Memory: 2 eligible entries omitted"));
}

#[test]
fn round2_preservation_project_knowledge_caps_warn() {
    let temp = tempfile::tempdir().unwrap();
    let _home = crate::support::paths::AidHomeGuard::set(temp.path());
    let knowledge = project::project_knowledge_dir(temp.path());
    std::fs::create_dir_all(&knowledge).unwrap();
    std::fs::write(
        knowledge.join("KNOWLEDGE.md"),
        (0..6)
            .map(|n| format!("- [rust memory {n}] — rust memory reference\n"))
            .collect::<String>(),
    )
    .unwrap();
    let store = Store::open_memory().unwrap();
    let args = RunArgs {
        prompt: "rust memory Caller brief".into(),
        ..Default::default()
    };
    let prompt = bundle(&store, &args, temp.path()).effective_prompt;
    assert!(prompt.contains(&args.prompt));
    assert!(prompt.contains("Knowledge: 1 eligible entries omitted"));
    assert!(warning(&store).contains("Knowledge: 1 eligible entries omitted"));
}
