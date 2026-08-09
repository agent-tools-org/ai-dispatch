## Findings

1. **Incorrect Midnight Fallback Direction** (Severity: High)
   **File:** `src/agent/codex_attribution.rs` (`find_session_file`)
   **Evidence:** The fallback logic uses `created_at - Duration::days(1)` (the previous day). However, the CLI creates the task and timestamps `created_at` *before* Codex executes and writes the rollout file. If a task is created just before midnight (e.g., 23:59:59 local), the rollout file might not be written until just after midnight (e.g., 00:00:01 local), placing it in the *next* day's directory (e.g., `sessions/2026/08/10`). Checking the previous day covers a physically impossible time-travel scenario and misses the actual midnight-crossing edge case. It should check `created_at + Duration::days(1)`.

2. **Intentional Loss of `ConfirmedBySuccess` for Missing Rollouts** (Severity: Medium)
   **File:** `src/agent/codex_attribution.rs` (`grade_completion_observation`)
   **Evidence:** In Round 1, if Codex lacked a rollout file, the code fell through to `grade_observation`, which could still grant `ConfirmedBySuccess` if a requested model succeeded. The new delta introduces a strict check (`map_or((None, None), ...)`) that intentionally returns `Unknown` for Codex runs missing a rollout (as verified by the new test `codex_without_rollout_stays_unknown_instead_of_confirming_request`). This alters the attribution behavior from Round 1, intentionally disabling fallback attribution for Codex.

## Question Answers

1. **Is the day-directory derivation correct, and does the fallback actually cover the edge it claims?**
   **FAIL.** The `DateTime<Local>` derivation is structurally correct. Files on disk confirm that Codex directories (`2026/08/09`) and filenames (`rollout-2026-08-09T11-59-40-...`) are named via Local time, regardless of the inner `turn_context` UTC payload timestamp (`2026-08-09T04:59...Z`). However, the previous-day-only fallback fails. Because file creation occurs *after* task creation, a midnight-crossing run lands in the *next* day's directory, which neither the exact-day match nor the previous-day fallback will read. 

2. **Does it still attribute a real run?**
   **PASS.** Tested against real task `t-152b375a` (created at `2026-08-09T11:59:38+07:00` with session ID `019fe4e4-1457-7483-8e43-37fb20700ec6`). The new logic successfully locates `~/.codex/sessions/2026/08/09/rollout-2026-08-09T11-59-40-...jsonl` on the same day and correctly parses `gpt-5.6-luna` from the first populated `turn_context`. For a task old enough that its rollout is rotated or deleted, `find_session_file` cleanly returns `None`, which the new logic safely translates into `(None, None)` (unknown model and source).

3. **Did the delta break anything that round 1 passed?**
   **PASS.** Diffing Round 1 (`79231671`) against the tip (`af2e04b7`) shows the explicit invariants hold:
   - **Other-agent attribution:** Agents other than Codex bypass the `AgentKind::Codex` block entirely, reaching `grade_observation` and retaining their previous logic.
   - **Null-stays-null:** If no model is found in the rollout, it returns `(None, None)`.
   - **`RecordedByCli` reaches only read models:** The use of `map_or` correctly bounds `AttributionSource::RecordedByCli` so it is exclusively paired with a successfully read model.

## Verdict

**FIX**

The midnight fallback must be corrected from `Duration::days(1)` subtraction to addition (or checking both directions) so that tasks starting at 23:59 can reliably resolve rollouts written at 00:00.

=== AID TASK t-b8f35093 DONE (exit 0) ===
