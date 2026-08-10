## Findings

1. **High — PTY input was written without checking agent capability.**
   `src/pty_watch.rs:583-590` now uses `accepts_interactive_input()` as the
   write-site guard around response, steer, and persisted-reply forwarding.
   Noninteractive agents leave queued messages and signal files untouched. The
   regression is covered by `pty_watch::write_tests::noninteractive_agent_leaves_queued_input_untouched`.

2. **High — Custom input capability was incorrectly derived from output format.**
   `src/agent/custom.rs:38-43,134-137` adds `interactive_input`, defaulting to
   `true` so existing custom agents retain their historical behavior. An
   explicit `interactive_input = false` opts out, independently of
   `streaming`; delegated OpenCode overlays receive the same capability.
   Agy and Grok remain classified as noninteractive by their one-shot print-mode
   adapters; Codex remains interactive but keeps its explicit no-idle-nudge
   override.

3. **Medium — Refusal messages did not describe the operation that failed.**
   `src/cmd/reply.rs:24-50,135-164` carries command context so `steer` reports
   that no steer message was queued, while `respond` reports that no response
   signal was written. Both paths resolve the task before writing anything.

4. **Medium — Deleted custom-agent definitions produced no recovery guidance.**
   `src/cmd/reply.rs:136-146` now identifies the missing custom agent and tells
   the caller to restore its TOML definition or stop and retry with an available
   agent. Both `steer` and `respond` have regression coverage.

## Verification

- `aid build` — passed with zero errors and warnings.
- Focused `aid test` runs passed for one-shot refusal, deleted-agent recovery,
  PTY no-consumption, custom capability independence, idle-nudge suppression,
  delegated custom capability, Codex delivery, reply queuing, and the audited
  PTY write-failure path.
- `aid test --test aid_guide_e2e -- official_guide_documents_steering_delivery_contract` — passed.
- No formatter was run. No PTY write-failure handling was changed.
