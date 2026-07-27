# Codex Final-Delivery Guard

## Status

Proposed design for the successful-exit-without-delivery incident observed on
2026-07-27. This document defines the delivery contract and test gate before
production code changes begin.

## Incident

Eight read-only Codex investigations covered issues #859 through #866. All
processes exited with code 0 and emitted `turn.completed`. Only #864 and #866
emitted a substantive `agent_message` after their final work event. The other
six consumed 8,871 to 14,030 output tokens but ended after a command or todo
event, leaving only earlier progress messages.

AID marked all eight tasks `done` because streaming completion currently trusts
the child exit code. The existing `HollowOutput` assessment did not catch the
six incomplete deliveries because it counts accumulated text; progress messages
already exceeded its 200-character threshold.

## Goal

A task must not become successfully delivered merely because its agent process
exited successfully. AID must:

1. distinguish final-delivery evidence from progress text;
2. prevent a hollow run from being reported as successfully delivered;
3. preserve the original investigation and usage evidence;
4. recover once through the same Codex session when safe;
5. terminate clearly if recovery also lacks a final deliverable.

## Non-goals

- Inferring whether every requested investigation step was completed.
- Classifying report quality or factual correctness.
- Adding model-specific output-token limits.
- Keeping legacy completion behavior behind a fallback.
- Changing successful behavior for non-Codex agents in the first patch.

## Delivery Contract

### Event classes

The Codex JSONL stream is reduced to these delivery facts:

- `Work`: command execution, file change, tool call, or todo update.
- `Message`: a non-empty completed `agent_message`.
- `TurnComplete`: `turn.completed`.
- `Error`: an explicit error event or unsuccessful process exit.

For each completed message, record whether a work event was observed before it
and whether another work event followed it.

### Successful delivery

A Codex run has final-delivery evidence when all conditions hold:

1. the process exits successfully;
2. a non-empty completed `agent_message` exists;
3. no `Work` event follows that message;
4. the message contains at least `MIN_FINAL_MESSAGE_CHARS` non-whitespace
   characters.

The length floor only rejects acknowledgements and fragments. Event ordering is
the primary signal. The constant must be centralized and initially set to 200
characters to match the existing substantive-output convention.

For write tasks, a committed or dirty scoped diff remains independent delivery
evidence for preserving work, but it does not manufacture a missing user-facing
final response. Verification and merge behavior must remain unchanged.

### Failed delivery

The process may exit successfully while delivery validation returns:

```text
MissingFinalDelivery
  last_work_kind
  last_work_sequence
  last_message_sequence
  last_message_chars
  output_tokens
```

This is an expected domain outcome, not a parser exception. It must be persisted
as `DeliveryAssessment::MissingFinalDelivery`.

`HollowOutput` remains for agents without structured final-message evidence.
Codex uses the stronger ordering-based assessment.

## State Transitions

The watcher remains responsible for process facts, not task success:

```text
process exit + parsed stream
        |
        v
DeliveryEvidence::validate()
        |
        +-- Delivered ----------------------> Done
        |
        +-- MissingFinalDelivery
                |
                +-- recovery allowed ------> child retry, original Failed
                |
                +-- recovery unavailable --> Failed
```

The original hollow attempt must transition `Running -> Failed`, with exit code
0 retained as a process fact. A child retry receives `parent_task_id` and owns
the eventual `Done` status. This avoids rewriting terminal history and matches
existing retry-chain behavior.

## Recovery

### Eligibility

Perform at most one automatic delivery recovery per retry chain when:

- the agent is Codex;
- the assessment is `MissingFinalDelivery`;
- an `agent_session_id` was captured from `thread.started`;
- no earlier chain member has a delivery-recovery marker;
- the task was not explicitly stopped;
- the configured task budget still permits another run.

If any condition fails, leave the task failed with a direct manual retry hint.

### Resume command

Codex command construction must use:

```text
codex exec resume --json <session-id> <recovery-prompt>
```

and retain the original model, directory, sandbox, writable-root, and output
options. The session ID is a validated UUID captured from Codex output, never a
free-form shell fragment.

### Recovery prompt

The recovery prompt is externalized as a constant:

```text
Your previous turn ended without a final deliverable. Do not perform more
investigation or call tools unless the existing evidence is insufficient to
avoid a factual error. Produce the requested final answer now using the evidence
already collected. Follow the original output and citation requirements.
```

The retry must not prepend the full original prompt because the resumed session
already contains it. This prevents duplicated context and another budget loss.

### Recovery result

- A valid final message marks the child task `Done`.
- A second missing final message marks the child `Failed`.
- No third automatic attempt is allowed.
- Hooks and webhooks report both attempts using their actual statuses.

## Component Boundaries

### New feature module

Add `src/delivery_guard.rs` with:

- `DeliveryEvidence`: typed facts accumulated from the stream;
- `DeliveryOutcome`: `Delivered` or `MissingFinalDelivery`;
- `observe_codex_jsonl`: parse only delivery-relevant ordering facts;
- `validate_codex_delivery`: pure validation logic.

The module must not access the database, spawn processes, or mutate task status.

### Codex adapter

`src/agent/codex.rs`:

- build `codex exec resume` when `RunOpts.session_id` is present;
- preserve existing flags in both new and resumed runs;
- continue capturing `thread.started` as `agent_session_id`.

`AgentKind::Codex` then opts into `supports_session_resume`.

### Watcher

`src/watcher.rs`:

- accumulate `DeliveryEvidence` while writing the unmodified JSONL log;
- return process completion and delivery outcome separately;
- never infer `Done` from exit code alone for Codex.

### Lifecycle

Add `src/cmd/run_delivery_recovery.rs`:

- persist the delivery assessment and diagnostic event;
- transition the hollow attempt to `Failed`;
- enforce the one-recovery chain invariant;
- construct and dispatch the resumed child task.

Keep orchestration out of the store mutation layer.

### Summary and UI

- `completion_summary.conclusion` must be empty when delivery is missing.
- `aid show` must display the last work kind, output-token count, and retry hint.
- Board and JSON output expose `missing_final_delivery` through the existing
  `delivery_assessment` field.
- Existing `hollow_output` records remain readable; no compatibility shim is
  added to runtime logic.

## E2E-First Acceptance Matrix

The first implementation commit adds failing tests for these flows:

| Flow | JSONL ending | Expected result |
|---|---|---|
| Normal report | work, long message, turn complete, exit 0 | `Done` |
| Incident reproduction | work, turn complete, exit 0 | original `Failed`, one resumed child |
| Stale progress message | message, work, turn complete, exit 0 | recovery required |
| Tiny trailing fragment | work, short message, turn complete, exit 0 | recovery required |
| Tool-free answer | long message, turn complete, exit 0 | `Done` |
| Agent error | work, error, exit non-zero | existing failure path |
| Recovery succeeds | hollow parent, resumed long message | parent `Failed`, child `Done` |
| Recovery repeats hollow | hollow parent and child | both `Failed`, no third task |
| Missing session ID | hollow run without thread event | `Failed`, manual retry hint |
| Write task with diff | file change, no final message, exit 0 | work preserved, task not `Done` |

The E2E fixture must execute a fake Codex binary through the normal CLI dispatch,
write real JSONL, persist task rows and events, and assert the retry chain. Unit
tests then cover ordering permutations and resume command construction.

## Implementation Slices

1. Add failing E2E fixtures for normal and missing-final Codex runs.
2. Add the pure delivery-evidence module and unit tests.
3. Gate Codex `Done` on validated delivery evidence.
4. Persist `MissingFinalDelivery` and correct summary/UI behavior.
5. Add Codex session resume command support.
6. Add one-shot automatic delivery recovery and retry-chain E2E tests.
7. Run targeted Rust tests and the relevant E2E suite. Do not run `cargo fmt`.

Each slice should remain below 400 changed lines. New source files require the
standard purpose, exports, and dependency header.

## Rollout and Observability

Record one structured event on detection:

```json
{
  "delivery_guard": "missing_final_delivery",
  "last_work_kind": "command_execution",
  "last_message_chars": 341,
  "output_tokens": 9095,
  "auto_recovery": "started"
}
```

Track:

- missing-final detections by agent and model;
- automatic recovery attempts and success rate;
- repeated missing-final failures;
- token cost of original and recovery turns.

Before release, replay the eight incident logs through the pure validator. The
expected result is six missing deliveries and two successful deliveries.

## Open Design Decision

Automatic recovery changes cost and task fan-out. The safe default for the first
release is one recovery for read-only Codex tasks only. Extending recovery to
write tasks should require a separate decision after observing read-only success
rate; detection still applies to both.
