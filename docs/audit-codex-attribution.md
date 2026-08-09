[MILESTONE] Diff main...fix/codex-model-attribution analyzed and changes verified
[MILESTONE] Queried aid.db and inspected real Codex rollout files on disk
[MILESTONE] Evaluated attribution logic, fallback paths, and performance characteristics

1. **Did this break something that already worked?**
PASS. The refactoring in `grade_completion_observation` explicitly checks `if info.model.is_some()` first, which perfectly preserves the `Echoed` attribution behavior for agents like `claude`, `cursor`, and `agy` that parse models from stdout. For non-Codex agents where `info.model.is_none()`, it falls through to `grade_observation(None, requested, succeeded)`, preserving the `ConfirmedBySuccess` fallback exactly as before.

2. **Does it attribute a REAL run, not a fixture?**
PASS. I queried `aid.db` for recent codex runs and found task `t-2fddfd38` with `agent_session_id` `019fe4e0-4d19-7552-ac5f-fa1f5a69ba69`. I located its real rollout file at `~/.codex/sessions/2026/08/09/rollout-2026-08-09T11-55-32-019fe4e0-4d19-7552-ac5f-fa1f5a69ba69.jsonl`. The parser logic (`value.pointer("/payload/model")` on `type: "turn_context"`) perfectly matches the real file's structure: `{"timestamp":"...","type":"turn_context","payload":{...,"model":"gpt-5.6-luna",...}}`. It correctly extracts `gpt-5.6-luna`.

3. **Does unknown stay null, and is the evidence grade honest?**
FAIL on the deliberate choice.
*   **Unknown stays null:** Yes, if the rollout file, `turn_context`, or `model` key is absent, the code explicitly returns `(None, None)` for Codex runs, intentionally bypassing the `ConfirmedBySuccess` fallback. 
*   **Evidence grade:** `RecordedByCli` is an honest and accurate description. It distinguishes a persisted out-of-band metadata record written by the CLI from streamed runtime stdout (`Echoed`).
*   **Deliberate choice (FAIL):** Bailing out and returning `None` immediately when the *first* `turn_context` lacks a model is incorrect. A session may have multiple turns (e.g., an initialization turn followed by an execution turn). The parser should continue scanning the file for the first `turn_context` that *does* contain a model, rather than discarding valid evidence from a later turn in the same session.

**What did I miss?**
*   **O(N) Unbounded Filesystem Traversal:** `find_session_file` recursively scans the *entire* `~/.codex/sessions/` directory (all years, months, and days) for every Codex task that completes. This is an unbounded disk traversal on the main execution path that will severely degrade performance over time. Since `thread_id` is a UUIDv7 (which encodes a timestamp) and tasks have a `created_at` date, the code should directly construct the path to today's (and maybe yesterday's) directory, rather than blindly iterating the whole tree.
*   **Double JSON Parsing:** The loop parses each line into a `serde_json::Value` in `is_turn_context` just to check the type, drops it, and then parses the exact same line into a `serde_json::Value` again in `model_from_turn_context`. This should be combined into a single parse.

**Overall:**
FIX / BLOCK. The unbounded O(N) filesystem traversal of the entire `sessions` directory on every task completion is a critical performance defect that will cause hangs, and bailing on the first `turn_context` without scanning subsequent turns incorrectly loses valid attribution.

=== AID TASK t-2d37cd10 DONE (exit 0) ===
