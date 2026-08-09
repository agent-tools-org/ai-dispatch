// Tests for delivery evidence: Codex JSONL ordering plus the plain-text report gate.
// Exports: module-scoped tests only.
// Deps: super::{DeliveryEvidence, DeliveryOutcome, looks_like_delivered_report}.

use super::{DeliveryEvidence, DeliveryOutcome};

const LONG_MESSAGE: &str = "A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail. A final report with enough detail.";

fn observe(evidence: &mut DeliveryEvidence, event: serde_json::Value) {
    evidence.observe_codex_jsonl(&event.to_string());
}

#[test]
fn accepts_substantive_message_after_work() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
    assert_eq!(evidence.validate(false), DeliveryOutcome::Delivered);
    assert_eq!(evidence.validate(true), DeliveryOutcome::Delivered);
}

#[test]
fn rejects_progress_message_followed_by_work() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    assert!(matches!(
        evidence.validate(false),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
    assert!(matches!(
        evidence.validate(true),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
}

#[test]
fn rejects_short_trailing_fragment_without_changes() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"done"}}));
    assert!(matches!(
        evidence.validate(false),
        DeliveryOutcome::MissingFinalDelivery { last_message_chars: 4, .. }
    ));
}

/// Incident t-346c5194: a commit follow-up's correct closing message is short
/// prose. Length floor waived when the diff is the deliverable; same text must
/// still fail without changes (fails if the waive is applied unconditionally).
#[test]
fn accepts_short_trailing_message_when_diff_is_deliverable() {
    let mut evidence = DeliveryEvidence::default();
    // Verbatim shape of the t-346c5194 closing message (< 200 chars, real prose).
    let short_message = "Committed all changes:\n\
`b9525d8f docs: investigate dead heartbeat decision counters`\n\
Worktree is clean. No source files, configs, or scripts were modified.";
    assert!(short_message.chars().count() < super::MIN_FINAL_MESSAGE_CHARS);
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":short_message}}));
    assert_eq!(evidence.validate(true), DeliveryOutcome::Delivered);
    assert!(matches!(
        evidence.validate(false),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
}

/// Delivery-shaped short prose without changes must still hit the length floor.
/// Fails if the waive ignores `produced_changes` (reverting the diff check).
#[test]
fn rejects_delivery_shaped_short_message_without_changes() {
    let mut evidence = DeliveryEvidence::default();
    let short_message = "Committed all changes.\nWorktree is clean.";
    assert!(short_message.chars().count() < super::MIN_FINAL_MESSAGE_CHARS);
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":short_message}}));
    assert!(matches!(
        evidence.validate(false),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
}

/// A stray edit plus a two-word fragment must still fail — non-empty is not a
/// delivery. Fails if the changed-task floor collapses to `chars > 0`.
#[test]
fn rejects_changed_task_with_non_delivery_fragment() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"ok mid"}}));
    assert!(matches!(
        evidence.validate(true),
        DeliveryOutcome::MissingFinalDelivery { last_message_chars: 6, .. }
    ));
}

/// Narration announcing the next tool call is not a delivery, even with a diff.
#[test]
fn rejects_changed_task_with_narration_only_trailing_message() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(
        &mut evidence,
        serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":"I will commit the remaining files."}}),
    );
    assert!(matches!(
        evidence.validate(true),
        DeliveryOutcome::MissingFinalDelivery { .. }
    ));
}

#[test]
fn rejects_138_char_final_message_without_changes() {
    let mut evidence = DeliveryEvidence::default();
    let short_message = "x".repeat(138);
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":short_message}}));
    assert!(matches!(
        evidence.validate(false),
        DeliveryOutcome::MissingFinalDelivery { last_message_chars: 138, .. }
    ));
}

/// Without a trailing agent message the guard must still fail even when the
/// diff exists — that is the "did work then died silently" case it catches.
#[test]
fn rejects_changed_task_with_no_trailing_message() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    assert!(matches!(
        evidence.validate(true),
        DeliveryOutcome::MissingFinalDelivery { last_message_chars: 0, .. }
    ));
}

#[test]
fn accepts_tool_free_answer() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
    assert_eq!(evidence.validate(false), DeliveryOutcome::Delivered);
}

#[test]
fn accepts_final_message_followed_by_todo_list_update() {
    let mut evidence = DeliveryEvidence::default();
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"command_execution"}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"agent_message","text":LONG_MESSAGE}}));
    observe(&mut evidence, serde_json::json!({"type":"item.started","item":{"type":"todo_list","items":[]}}));
    observe(&mut evidence, serde_json::json!({"type":"item.updated","item":{"type":"todo_list","items":[]}}));
    observe(&mut evidence, serde_json::json!({"type":"item.completed","item":{"type":"todo_list","items":[]}}));
    assert_eq!(evidence.validate(false), DeliveryOutcome::Delivered);
}

/// Verbatim shape of the t-f2f1e7c1 capture: agy died mid-audit and left only the
/// lines announcing each tool call, which aid then persisted as the audit report.
const NARRATION_CAPTURE: &str = "I will start by checking the list of permissions to see what actions and directories are available to us.\n\
I will run `pwd` to identify the current working directory of our workspace.\n\
I will run `git diff main..HEAD` to get the list of changes made in the branch.\n\
I will output the full git diff to a file in the scratch folder so we can read it without truncation.\n\
I will inspect the indexer snapshot file `crates/sr-indexer/src/snapshot.rs` to answer Q2.\n\
[MILESTONE] Analyzed Q2 regarding serialization determinism across indexer restarts.\n";

#[test]
fn rejects_pre_tool_narration_capture() {
    assert!(!super::looks_like_delivered_report(NARRATION_CAPTURE));
}

#[test]
fn accepts_markdown_report() {
    let report = format!("## Findings\n\nQ1 PASS. {LONG_MESSAGE}");
    assert!(super::looks_like_delivered_report(&report));
}

#[test]
fn accepts_prose_report_without_headings() {
    assert!(super::looks_like_delivered_report(LONG_MESSAGE));
}

/// The report instruction explicitly asks for this when an audit is clean, so the
/// shortest valid report must not be mistaken for a missing one.
#[test]
fn accepts_the_shortest_valid_report() {
    assert!(super::looks_like_delivered_report("## Findings\nNo findings."));
}

#[test]
fn rejects_short_text_without_a_heading() {
    assert!(!super::looks_like_delivered_report("done"));
}

#[test]
fn narration_followed_by_a_real_report_still_counts_as_delivered() {
    let mixed = format!("{NARRATION_CAPTURE}\n## Findings\n\n{LONG_MESSAGE}");
    assert!(super::looks_like_delivered_report(&mixed));
}

/// A heading must not launder narration: an agent that died after printing a plan
/// heading has still delivered nothing.
#[test]
fn rejects_narration_under_a_heading() {
    let planning = format!("# Investigation Plan\n{NARRATION_CAPTURE}{NARRATION_CAPTURE}");
    assert!(!super::looks_like_delivered_report(&planning));
}

/// Audit t-b4423393: a report may legitimately end on a sentence that starts like an
/// announcement. First-person-plural is prose, not a tool call.
#[test]
fn accepts_report_ending_on_a_forward_looking_sentence() {
    let report = "## Findings\n\
No vulnerabilities were found in the codebase.\n\
We will monitor the application logs for any errors.\n";
    assert!(super::looks_like_delivered_report(report));
}

/// Audit t-b4423393: bulk alone must not pass raw tool output off as a report.
#[test]
fn rejects_raw_tool_output_trailing_a_narration_line() {
    let capture = "I'll list the directory to see the files.\n\
total 0\n\
-rw-r--r--  1 user  group    0 Jul 31 22:00 Cargo.toml\n\
-rw-r--r--  1 user  group    0 Jul 31 22:00 src/lib.rs\n\
-rw-r--r--  1 user  group    0 Jul 31 22:00 src/main.rs\n\
-rw-r--r--  1 user  group    0 Jul 31 22:00 tests/integration.rs\n\
-rw-r--r--  1 user  group    0 Jul 31 22:00 docs/readme.md\n";
    assert!(!super::looks_like_delivered_report(capture));
}

/// Audit t-af06afd3 case 4: a Chinese report ends on `。`, not `.`.
#[test]
fn accepts_a_report_in_chinese() {
    let report = "本次代码审计工作已经全部完成。我们对所有的边界条件进行了详细的检查，并确认了1680个单元测试全部顺利通过。\
我们建议可以立即合并当前的分支并部署上线。为了确保系统的长期稳定性，我们还建议在后续的开发中继续保持单元测试的完整覆盖率，\
并定期进行自动化的安全审计工作。同时，我们也已经将所有的审计日志和详细的测试报告保存到了指定的归档目录中，\
方便团队其他成员随时查阅和核对。审计过程中重点复核了快照摘要跳过逻辑在冷启动、部分失败以及进程重启这三种情形下的行为，\
均未发现会导致快照被错误跳过的路径。此外我们还确认了丢弃计数所使用的互斥锁只出现在丢弃分支上，\
不会影响正常的发布路径，因此不存在额外的锁竞争风险。";
    assert!(report.chars().count() >= super::MIN_FINAL_MESSAGE_CHARS);
    assert!(super::looks_like_delivered_report(report));
}

/// Audit t-af06afd3 case 1: a bullet report need not end any line with a period.
#[test]
fn accepts_a_bullet_report_without_sentence_punctuation() {
    let report = "- Digest skip is safe on cold start, no recorded digest means no skip\n\
- RecentlyUpdated scope keeps its own digest key, so a full snapshot still applies\n\
- Two replicas share one process-global map, which is per-process and therefore fine\n\
- Drop accounting takes the mutex only on the drop path, not the publish path\n";
    assert!(super::looks_like_delivered_report(report));
}

/// Audit t-af06afd3 case 3: a whole report on one line, opening like an announcement.
#[test]
fn accepts_a_single_line_report_that_opens_like_narration() {
    let report = "I will present the final report: we fixed every bug in the codebase, cleaned up the \
compiler warnings, ran the full test suite, and verified that all 1680 unit tests pass without \
any errors or ignored cases.";
    assert!(report.chars().count() >= super::MIN_FINAL_MESSAGE_CHARS);
    assert!(super::looks_like_delivered_report(report));
}

#[test]
fn rejects_narration_behind_discourse_markers() {
    let text = "First, I'll search the repository for the affected call sites.\n\
Next, I will read the snapshot module to confirm the encoding order.\n\
Then I'll check whether the digest guard agrees with the content hash.\n\
Let's inspect the drop accounting path afterwards.\n\
We will finish by summarizing the three answers.\n";
    assert!(!super::looks_like_delivered_report(text));
}
