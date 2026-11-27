// Project stage through the real prompt bundle builder: audit bar and work hint.
// Unset and stable stages must leave the resolved prompt byte-identical.
use super::*;

fn resolved(prompt: &str, read_only: bool, stage: Option<project::ProjectStage>) -> String {
    let temp = crate::support::test_env::TestHome::new();
    let store = Store::open_memory().unwrap();
    store
        .insert_waiting_task("t-stage", "codex", prompt, None, None, None, None, None, None, false, false)
        .unwrap();
    let args = RunArgs { prompt: prompt.into(), read_only, ..Default::default() };
    let project = project::ProjectConfig { id: "stage".into(), stage, ..Default::default() };
    build_prompt_bundle(&store, &args, &AgentKind::Codex, None, &[], "t-stage", Some(&project), Some(temp.path()))
        .unwrap()
        .effective_prompt
}

const AUDIT: &str = "Read-only audit of commit abc123. Do not edit files.";
const IMPLEMENT: &str = "Implement the parser and add tests.";

#[test]
fn unset_and_stable_stage_resolve_identical_prompts() {
    for (prompt, read_only) in [(AUDIT, true), (IMPLEMENT, false)] {
        let unset = resolved(prompt, read_only, None);
        assert_eq!(resolved(prompt, read_only, Some(project::ProjectStage::Stable)), unset);
        assert!(!unset.contains("Project stage:"));
    }
}

#[test]
fn prototype_audit_gets_the_lowered_bar_and_work_hint() {
    let prompt = resolved(AUDIT, true, Some(project::ProjectStage::Prototype));
    assert!(prompt.contains("Project stage: prototype (idea or draft)."));
    assert!(prompt.contains("only a high-impact finding justifies FIX or BLOCK"));
    assert!(prompt.contains("- Project stage: prototype. Favor reaching"));
}

#[test]
fn experimental_implementation_gets_only_the_work_hint() {
    let prompt = resolved(IMPLEMENT, false, Some(project::ProjectStage::Experimental));
    assert!(prompt.contains("- Project stage: experimental. Keep the main path working"));
    assert!(!prompt.contains("Project stage: experimental (development or experiment)."));
}
