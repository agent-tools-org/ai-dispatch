# Audit: Task Result Recognition

## Scope

This audit traces every production path that can set a task to `Failed`,
override a successful process exit, trigger a result-driven retry, or classify
delivery and verification. It also records adjacent hard-coded heuristics that
do not control terminal status.

The governing rule is:

> AID may recognize facts exposed by a process, structured agent protocol, or
> explicit user contract. It must not infer success or failure from response
> length, prose shape, headings, nearby keywords, or prompt wording.

## Incident Finding

Task `t-5d7f1e71` requested `Reply with the single word: ok`. Codex emitted a
completed `agent_message` containing `ok`, emitted `turn.completed`, and exited
with code 0. AID then changed the task to `Failed` because the message had 2
characters and the delivery guard required 200.

The 200-character value came from an earlier hollow-output convention. It was
not derived from the Codex protocol, the task contract, or observed correctness.
The database currently contains 9 Codex tasks with exit code 0 and a non-empty
final message below that old floor that were recorded as
`missing_final_delivery`. This count identifies exposure to the rule; it does
not claim that every historical task was otherwise correct.

## Corrected Result Contract

| Signal | Meaning | May change terminal status? |
|---|---|---:|
| Process exit code | Whether the agent process completed successfully | Yes |
| Structured `error` / `is_error: true` | Agent protocol reported failure | Yes |
| Structured terminal cancellation | Agent protocol reported no completed delivery | Yes |
| Non-empty final message after the last work event | Codex delivered a response | Yes |
| Explicit result file present and non-empty | Caller-requested artifact was delivered | Yes |
| Explicit result file absent or empty | Caller-requested artifact was not delivered | Yes |
| Verify process exit 0 / non-zero | Explicit verification command passed / failed | Yes |
| Verify spawn, pipe, reader, or wait error | Verification did not execute reliably | Records infrastructure failure |
| Verify timeout | Verification is inconclusive | No task failure |
| Response character or word count | Display fact only | No |
| Markdown headings or prose shape | Content formatting only | No |
| Keywords found in ordinary model prose | Unattributed text | No |
| Prompt wording guessed as an acceptance criterion | Unstructured intent | No |

## Removed Unsupported Decisions

1. **Codex delivery length floor.** `MIN_FINAL_MESSAGE_CHARS = 200` and the
   associated report-shape checks were deleted. Delivery now uses only final
   message presence and event ordering.
2. **Hollow-output length grading.** One character of real output is content;
   whitespace-only output is empty. This assessment remains informational and
   does not fail a completed task.
3. **Short-message filtering.** Output extraction no longer discards agent
   messages below 50 characters.
4. **Quota plus prose grading.** Quota handling receives an explicit delivery
   fact from each adapter. It no longer promotes text to a deliverable because
   it is long or report-shaped.
5. **Qwen failure substrings.** Model prose containing `API Error:` is not a
   failure. Only explicit error/result fields and the captured terminal API
   error result shape are failures.
6. **Unknown result subtypes.** Unknown non-error result subtypes remain
   successful instead of being failed by an invented allowlist.
7. **Verify-output keyword classification.** `sccache`, compiler, test, disk,
   Docker, and panic keywords no longer decide whether a non-zero verify exit is
   a code failure or infrastructure failure.
8. **Prompt-derived file promises.** Natural-language phrases such as `Create a
   new file:` no longer become hidden verify gates. Artifact requirements must
   be passed through an explicit result-file contract.
9. **Model-health character ceiling.** The 500-character error limit was
   removed. Plain errors are accepted only from stderr with captured error
   prefixes; task logs require a structured error event.
10. **Checklist proximity scan.** The 200-character keyword window was replaced
    by an injected, exact `CHECKLIST N: CONFIRMED|REJECTED` response contract.
11. **Hung-retry progress floor.** Retry eligibility no longer depends on an
    arbitrary minimum of six progress events. Retry counts remain explicit
    operational limits.
12. **Short-prompt skill suppression.** A named skill is always injected. Prompt
    length no longer decides whether its methodology is silently omitted.
13. **Cheap-model smart routing.** Automatic model downgrade now requires the
    caller's declared `trivial` or `simple` difficulty. Prompt length and word
    count no longer make that decision.
14. **Invented judge defaults.** A judge response without an exact first-line
    `PASS:` / `RETRY:` verdict is inconclusive instead of passing. A peer review
    without an explicit 1–10 score is inconclusive instead of receiving 5.
15. **Mostly-JSON output guessing.** Log cleanup no longer treats an arbitrary
    majority of JSON-looking lines as a structured transcript. It cleans output
    only when every non-empty line satisfies the JSON object contract.
16. **Command Code success allowlist.** Result envelopes no longer fail merely
    because a subtype or stop reason is new. Only explicit error flags and
    captured non-delivery values such as cancellation or `max_turns` fail.

## Remaining Thresholds by Category

The repository still contains numeric constants, but they are not all result
judges:

- **Operational safeguards:** configured idle, first-token, maximum-duration,
  retry, concurrency, cost, and prompt-token limits. These control resource use
  and are observable as timeout, stop, or budget facts.
- **Display bounds:** excerpts, board previews, log tails, and diagnostic
  truncation. These affect presentation only.
- **Advisory routing:** task-category inference and capability scores used by
  `aid advise` and recommendation hints. They remain predictions, not success
  criteria. Explicit `--kind`, difficulty, model, budget, verify, and result
  file declarations take precedence.
- **Historical delivery assessments:** `empty_diff` and `hollow_output` remain
  readable and informational. They do not turn `Done` into `Failed`.

Any future terminal-state rule must name its authoritative source: process
status, a documented protocol field, or an explicit caller declaration. If no
such source exists, the system must report the observation separately and leave
the task result unchanged.

## Regression Coverage

The regression suite covers an exact `ok` Codex answer end to end, missing final
messages, explicit missing result files, advisory auto-generated result files,
short output persistence, quota-with-delivery, unknown result subtypes, Qwen
prose false positives, verify exit semantics, prompt-prose non-contracts,
model-health channel boundaries, and exact checklist responses.
