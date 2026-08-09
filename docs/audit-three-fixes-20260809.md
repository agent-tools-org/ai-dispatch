[MILESTONE] Fetched and read diffs for all three branches
[MILESTONE] Audited budget-is-a-preference for budget short-circuiting and hard bail removal
[MILESTONE] Audited delivery-guard-short-delivery for substantive guard bypasses
[MILESTONE] Audited codex-resume-durable-home for sandbox regressions and milestone correctness

## Findings

### 1. Budget is a Preference
FAIL
- **Broke existing behavior:** By changing the hard bail to a warning, `explicit_agent` now returns `Ok((agent, None))`. If a caller requests `--budget free --agent claude` (which has no free models), it warns but proceeds to dispatch Claude's default premium model (Opus at $90/M), violating the financial constraint and spending money. (Evidence: `src/cmd_dispatch/run_profile.rs:86-94`)
- **Fix claims:** FAIL. The claim that it "picks the cheapest model in tier" is false globally. `model_for_task_budget` short-circuits on the first matched tier. For `--budget cheap` on `OpenCode`, it hits the `cheap` tier and selects `glm-5.2` ($2.36) without ever checking the `free` tier (`deepseek-v4-flash-free` at $0.00). The unknown-tier last resort does not pick an unservable model (Grok successfully serves `grok-4.5`). (Evidence: `src/model_catalog.rs:232-242`, `src/model_catalog_data.rs:138-142`)
- **Tests real:** PASS. `cheap_budget_dispatches_grok_with_unknown_tier_model` fails if reverted because the old implementation ignored the `unknown` tier.

### 2. Delivery Guard Short Delivery
FAIL
- **Broke existing behavior:** Waiving the floor when `produced_changes` is true breaks the safety invariant. An agent that makes a stray edit (`command_execution`) then dies mid-work with a two-word fragment (e.g., "I will...") now incorrectly passes the guard because `last_message_chars > 0` and `message_is_last` are true. (Evidence: `src/delivery_guard.rs:83-90`)
- **Fix claims:** PASS. The floor is waived for diffs, but this breaks the guard as detailed above.
- **Tests real:** FAIL. `rejects_changed_task_with_no_trailing_message` (0 chars) and `rejects_short_signoff_when_task_produced_no_changes` (20 chars) would both PASS if the fix were reverted, as the original `MIN_FINAL_MESSAGE_CHARS` check would still correctly reject them. (Evidence: `src/delivery_guard_tests.rs:147-156`, `175-182`)

### 3. Codex Resume Durable Home
FAIL
- **Broke existing behavior:** `apply_codex_home_env` overwrites `CODEX_HOME` with the host's real home. For sandbox/container runs, this forces paths outside the isolated `$HOME`, breaking them. Consequently, `durable_session_rollout_exists` searches the real home and drops valid session IDs that were correctly saved inside the isolated sandbox home. Additionally, `strip_suffix(session_id)` combined with `ends_with('-')` incorrectly matches substrings (e.g., session ID `123` matches `rollout-session-abc-123.jsonl`), allowing invalid sessions to pass. (Evidence: `src/agent/env.rs:174`, `src/agent/codex.rs:28-32`, `src/agent/codex.rs:52-54`)
- **Fix claims:** FAIL. Background tasks do not record a milestone. In `src/background.rs`, `opts.session_id` is hardcoded to `None`, completely bypassing `resume_fallback_needed` and silently starting a fresh session without a milestone. (Evidence: `src/background.rs:155-171`)
- **Tests real:** PASS. `build_command_starts_fresh_when_saved_rollout_is_missing` fails if reverted since the old code would attempt to resume.

OVERALL: BLOCK

what did I miss?

=== AID TASK t-62b9c042 DONE (exit 0) ===
