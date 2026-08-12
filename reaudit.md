[MILESTONE] Analyzed Question 1: FAIL 1 is closed.

[MILESTONE] Analyzed Question 2: FAIL 2 and FAIL 3 are closed.

## Findings

**Overall Verdict:** BLOCK

### 1. (PASS) Q2: The narrowed hold reaches the read path

**Evidence:**
The fix successfully reads the narrowed provider marker and holds the route.
- The `resolve_agent_setup` function (`src/cmd/run_dispatch_resolve.rs:234`) now calls `rate_limit::dispatch_blocking_hold_for_model` before attempting to dispatch.
- `dispatch_blocking_hold_for_model` correctly derives the group (the provider string) via `model_group`, constructs the group path (`group_marker_path`), and checks `.aid/rate-limit-opencode--<provider>`.
- If active, it cascades the agent entirely using `held::switch_model_held_route`, which in turn delegates to `skip_held_to_fallback`. The fallback itself is verified against its own global hold (`dispatch_blocking_hold`) before selection, safely advancing past chained outages.

### 2. (PASS) Q3: Unknown attribution and Ollama leakage

**Evidence:**
- **Unknown Attribution (FAIL 2):** The `named_opencode_provider` now filters out the literal `"unknown"` (via `.filter(|provider| !provider.eq_ignore_ascii_case("unknown"))`). A refusal containing `"provider": "unknown"` causes `group_from_refusal` to return `None`, forcing `mark_rate_limited_for_message` to correctly write an agent-wide marker (`.aid/rate-limit-opencode`), stopping dispatch entirely as desired.
- **Ollama Leakage (FAIL 3):** An Ollama refusal now extracts `"ollama"` as the provider and writes `.aid/rate-limit-opencode--ollama`. Because the hold is localized specifically to Ollama, dispatch will successfully cascade from it to other providers, safely isolating the local endpoint without crippling unrelated services.

### 3. (FAIL) NEW RISK: Breaking Cursor's group fallback and double-switching

**Evidence:**
The patch introduces a severe regression for multi-tier agents by intercepting group holds *before* the intra-agent fallback logic runs.
- **Double Switch and Cursor Breakage:** Before the patch, a group hold on Cursor's `premium` pool was caught by `healthy_model_for`, cleanly switching the requested model from `gpt-5.4-high` to `auto` while keeping the Cursor agent. The new `dispatch_blocking_hold_for_model` runs *first*, sees the premium group is held, and unconditionally calls `switch_model_held_route`. This completely abandons Cursor and triggers a full agent cascade (e.g., to Codex).
- **Bogus State & Warnings:** If a cascade lands *on* an agent with groups (e.g., OpenCode -> Cursor), `switch_model_held_route` mutates `agent_kind` to `Cursor` and sets `effective_model` to `None`. The execution then continues to `healthy_model_for(Cursor, None)`. Because `model_group(Cursor, None)` returns `None`, it picks the first healthy group's model (e.g., `composer-2.5`). The resolver then incorrectly emits `aid_warn!("[aid] cursor model group exhausted; switching (default) -> composer-2.5")` — an alarming warning for an exhausted group that was never actually requested nor held. 
- **The closure argument change:** Changing `custom_name` to `custom_agent_name.as_deref()` was forced by the authors' own insertion. In the original code, `custom_name` was correctly bound *after* the only cascade mutator. The authors inserted `switch_model_held_route` between the binding and the closure, rendering `custom_name` stale. They fixed the shadowing but left the architecture broken.

=== AID TASK t-b72cb67c DONE (exit 0) ===
