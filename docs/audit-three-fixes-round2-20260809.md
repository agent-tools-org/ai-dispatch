## Findings

1. **FAIL — sandbox/container resume regression.** `src/agent/codex.rs:63-71` always scans the real home’s `.codex/sessions`. Sandbox/container runs intentionally do not receive durable `CODEX_HOME` (`src/agent/env.rs:115-121`, `src/cmd/run_dispatch_execute.rs:214-220`), while the wrapper sets `HOME=/root` (`src/sandbox.rs:62-66`). A rollout existing only in the isolated home is therefore missed, and `src/agent/codex.rs:139-153` silently starts a fresh session. This is a regression for resumable isolated runs.

2. **FAIL — filename matching is still suffix-based.** `src/agent/codex.rs:74-92` accepts this exact case:

   `session_id = 019e3e49-6b83-7563-a3d8-b51a3a716dd1`

   `rollout-2026-08-09T17-20-31-extra-019e3e49-6b83-7563-a3d8-b51a3a716dd1.jsonl`

   The UUID gate blocks the old short-ID example, but does not enforce an exact session-ID boundary. The unchanged attribution matcher has the same behavior at `src/agent/codex_attribution.rs:96-101`.

3. **FAIL — non-UUID IDs are silently discarded.** `src/agent/codex.rs:422-434` accepts any string from input such as `{"type":"thread.started","thread_id":"session-1"}` and persists it through `src/watcher/stream.rs:118-133`. `is_uuid_session_id` rejects it at `src/agent/codex.rs:95-103`; resume then falls back without an error at `src/agent/codex.rs:139-153`. The repo has no Codex-specific validation or non-UUID guarantee. Existing Codex fixtures use UUIDs, but the external CLI contract is not established here.

4. **PASS — budget selection fix is real.** `src/model_catalog.rs:238-243` pools `cheap` and `free` tiers and chooses the lowest total price, so OpenCode selects `opencode/deepseek-v4-flash-free` instead of `opencode/glm-5.2`. The shared consumers route through this selector, and coverage exists at `src/model_catalog_tests.rs:74-85`.

5. **PASS — real-home forcing was removed for sandbox/container runs.** `src/agent/env.rs:115-121` requires Codex plus neither sandbox nor container before applying durable `CODEX_HOME`; both foreground and background callers use that guard.

The delivery-guard branch is unrelated to these three defects; its short-delivery validation is wired at `src/delivery_guard.rs:169-201` and `src/watcher.rs:155-162`.

**Overall: BLOCK**

## Open Questions

The current Codex CLI’s guaranteed session-ID format and whether isolated containers persist rollouts could not be verified from repository code. No branch tests were run during this read-only audit.

What did I miss?