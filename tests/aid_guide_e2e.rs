// E2E coverage for the release-managed official AID guide skill.
// Verifies installation, reference integrity, and public-command coverage.
// Deps: compiled aid binary and tempfile.

use tempfile::TempDir;

mod common;
use common::aid_cmd_in;

#[test]
fn official_guide_covers_every_public_command() {
    let aid_home = TempDir::new().unwrap();
    let init = aid_cmd_in(aid_home.path()).arg("init").output().unwrap();
    assert!(init.status.success());
    let guide_dir = aid_home.path().join("skills/aid-guide");
    let skill = std::fs::read_to_string(guide_dir.join("SKILL.md")).unwrap();
    let command_index =
        std::fs::read_to_string(guide_dir.join("references/command-index.md")).unwrap();
    let help = aid_cmd_in(aid_home.path()).arg("--help").output().unwrap();
    assert!(help.status.success());

    for command in public_commands(&String::from_utf8_lossy(&help.stdout)) {
        let documented = format!("`aid {command}`");
        assert!(
            command_index.contains(&documented),
            "official guide does not document public command: {command}"
        );
    }
    for reference in skill_references(&skill) {
        assert!(
            guide_dir.join(reference).is_file(),
            "official guide links missing reference: {reference}"
        );
    }
}

#[test]
fn official_guide_documents_prompt_only_audit_dispatch() {
    let dispatch = include_str!("../default-skills/aid-guide/references/dispatch.md");

    assert!(dispatch.contains("`read-only audit`"));
    assert!(dispatch.contains("`read-only comparative audit`"));
    assert!(dispatch.contains("`read-only cross-audit`"));
    assert!(dispatch.contains("`read-only re-audit`"));
    assert!(dispatch.contains("still permits writing the task result file"));
    assert!(dispatch.contains("refused before a task row is created"));
    assert!(dispatch.contains("`add an audit log`"));
    assert!(dispatch.contains("`make changes to the read-only audit logic`"));
    assert!(dispatch.contains("`do not modify` or `without modifying`"));
    assert!(dispatch.contains("independent of dirty-worktree enforcement"));
    assert!(dispatch.contains("`--result-file` controls report formatting and delivery"));
}

#[test]
fn official_guide_documents_watcher_safeguards() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("configured idle, hung-task, cost, and maximum-duration safeguards"));
    assert!(operations.contains("`--idle-timeout SECS`"));
    assert!(operations.contains("`--timeout SECS`"));
    assert!(operations.contains("Repeated activity is not itself a stop condition"));
    // The guide must not re-acquire the two inaccuracies the removal audit caught:
    // idle is refreshed by meaningful raw output aid cannot parse, and --timeout
    // is activity-aware, not a hard cap.
    assert!(operations.contains("Meaningful text"));
    assert!(operations.contains("refreshes the liveness clock"));
    assert!(operations.contains("activity-aware rather than a hard wall-clock cap"));
}

#[test]
fn official_guide_documents_grouped_tui_controls() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("The TUI shows every project grouped by project"));
    assert!(operations.contains("h/l"));
    assert!(operations.contains("Space"));
    assert!(operations.contains("/"));
    assert!(operations.contains("r"));
    assert!(operations.contains("CLI keeps its current-project default with --all"));
    assert!(!operations.contains("The TUI mirrors this default and toggles it with `P`."));
}

#[test]
fn official_guide_documents_steering_delivery_contract() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("`steer` is refused for the one-shot print-mode `agy` and `grok` CLIs"));
    assert!(operations.contains("aid reports the limitation"));
    assert!(operations.contains("steer message"));
    assert!(operations.contains("Codex steering remains supported"));
    assert!(operations.contains("`respond` is refused for those same one-shot CLIs"));
    assert!(operations.contains("no response signal was written"));
}

#[test]
fn official_guide_documents_event_fallback_coverage() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("`aid export --sharegpt` falls back to"));
    assert!(operations.contains("preserve tool calls, file reads, and file writes"));
    assert!(operations.contains("only edited or only read files is still represented"));
}

#[test]
fn official_guide_documents_retry_worktree_safety() {
    let operations = include_str!("../default-skills/aid-guide/references/task-operations.md");

    assert!(operations.contains("When the recorded linked worktree still exists, retry reuses it"));
    assert!(operations.contains("original repository checkout as its anchor"));
    assert!(operations.contains("genuinely checked out in the checkout that dispatched the task"));
    // Issue: a stalled task's own worktree lease refused its retry. The guide
    // must document that retry supersedes a non-terminal task's live worker.
    assert!(operations.contains("supersedes that task's own run"));
    assert!(operations.contains("stops the still-live worker first"));
    assert!(operations.contains("If the worker cannot be stopped, the retry is refused"));
    assert!(operations.contains("genuinely held by a different live task"));
}

#[test]
fn official_guide_documents_declared_profiles_and_advice() {
    let dispatch = include_str!("../default-skills/aid-guide/references/dispatch.md");
    let collaboration = include_str!("../default-skills/aid-guide/references/collaboration.md");
    let configuration = include_str!("../default-skills/aid-guide/references/configuration.md");

    assert!(dispatch.contains("`aid advise`"));
    assert!(dispatch.contains("without launching an agent or writing the task store"));
    assert!(dispatch.contains("--difficulty complex --budget premium --urgency urgent --rigor critical"));
    assert!(dispatch.contains("`aid run auto`") && dispatch.contains("hard errors"));
    assert!(collaboration.contains("declared `difficulty`, `budget`, `urgency`, and `rigor`"));
    assert!(collaboration.contains("`auto` and empty agent are rejected"));
    assert!(configuration.contains("`require_task_profile = true`"));
}

#[test]
fn official_guide_documents_recursive_delegation() {
    let dispatch = include_str!("../default-skills/aid-guide/references/dispatch.md");

    assert!(dispatch.contains("## Recursive delegation"));
    assert!(dispatch.contains("`AID_TASK_DEPTH`"));
    assert!(dispatch.contains("dispatch beyond depth `2` is refused"));
    assert!(dispatch.contains("`--bg` is refused"));
    assert!(dispatch.contains("may re-enter the same worktree"));
}

fn public_commands(help: &str) -> Vec<String> {
    help.lines()
        .skip_while(|line| *line != "Commands:")
        .skip(1)
        .take_while(|line| !line.is_empty())
        .filter_map(|line| line.split_whitespace().next())
        .filter(|command| *command != "help")
        .map(str::to_string)
        .collect()
}

fn skill_references(skill: &str) -> Vec<&str> {
    skill
        .split('(')
        .filter_map(|part| part.split_once(')'))
        .map(|(path, _)| path)
        .filter(|path| path.starts_with("references/"))
        .collect()
}
