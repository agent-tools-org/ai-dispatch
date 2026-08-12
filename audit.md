## Findings

**Overall Verdict:** BLOCK

### 1. (FAIL) Q2: The narrowed hold fails to reach the read path

**Severity:** High
**Files:** `src/cmd/run_dispatch_resolve.rs`, `src/agent/model_group.rs`
**Evidence:** 
The write path successfully records the narrowed hold by calling `mark_group_rate_limited` (creating `.aid/rate-limit-opencode--nvidia`). However, the dispatch read path completely ignores it.
During dispatch (`run_dispatch_resolve.rs`), group routing evaluates `agent::model_group::healthy_model_for`. `healthy_model_for` relies on `groups_for_agent(agent)`. Since `AgentKind::OpenCode` is missing from the match arms in `groups_for_agent`, it hits the `_ => &[]` fallback. With an empty group list, `healthy_model_for` bails out (`if groups.is_empty() { return None; }`) and never executes the closure containing the `is_group_rate_limited` check. Meanwhile, the global `is_rate_limited` check ignores group markers. The route stays fully active despite the file existing on disk.

### 2. (FAIL) Q3: Unknown attribution and data fabrication

**Severity:** Medium
**Files:** `src/rate_limit.rs`, `src/agent/model_group.rs`
**Evidence:**
- **Fabricated `recovery_at`:** In `parse_iso_recovery_time` (`src/rate_limit.rs`), an ISO timestamp found in the refusal is parsed into a `DateTime` and shifted to the system's local timezone (`.with_timezone(&Local).naive_local()`). It is then formatted via `format_recovery` (e.g., `Jan 02, 2099 10:04 AM`). This constructs a localized string that was not literally present in the provider's refusal message.
- **Unattributed Refusals Marking Less:** If an OpenCode refusal has no model but happens to contain a key like `provider` (e.g., `{"error": "limit", "provider": "unknown"}`), `named_opencode_provider` blindly extracts `"unknown"`. This causes `mark_rate_limited_for_message` to write a narrow group marker (`rate-limit-opencode--unknown`) instead of falling back to the global `mark_rate_limited`. The global agent avoids the hold, and the system continues routing tasks to it.
- **Ollama Quota Leakage:** Yes, ollama is treated as a quota-bearing provider. If the model is `"ollama/llama3"`, `provider_from_model` extracts `"ollama"`. Because `has_grouped_quota(OpenCode)` unconditionally returns true, any matched or manual refusal on that task will write `.aid/rate-limit-opencode--ollama`, treating the local unmetered endpoint as if it were a bounded cloud provider.

### 3. (PASS) Q1: Quota marking for other agents remains intact

**Severity:** None (Correct)
**Files:** `src/agent/model_group.rs`
**Evidence:**
- The behavior for agents other than OpenCode (e.g., Cursor, Antigravity) is byte-for-byte unchanged at runtime.
- In `has_grouped_quota`, the short-circuit `agent == AgentKind::OpenCode` leaves the old `!groups_for_agent(agent).is_empty()` logic exactly as it was.
- In `model_group`, the return type's lifetime was relaxed from `&'static str` to `&'a str` to allow returning a substring of the model name. For non-OpenCode agents, the function still returns static string constants (like `AUTO_GROUP`), which perfectly coerces to the `'a` lifetime without altering program behavior.

=== AID TASK t-c537498a DONE (exit 0) ===
