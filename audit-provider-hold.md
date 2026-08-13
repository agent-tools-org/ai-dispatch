I'll audit commit 846c14d0 as requested — read-only, no builds or test runs.[MILESTONE] Traced the diff and the full opencode refusal pipeline (parse_json_event → watcher → dispatch gate)

[MILESTONE] Grepped all captured task logs for real opencode error-event shapes and providerID occurrences

Final check on the changed function's construction sites before writing up:## Findings

### 1. The agent-wide hold is still written on the same line — the change cannot alter end-to-end behaviour (BLOCKING)

`opencode` is `streaming() == true` and `needs_pty() == true`, so every stdout line goes through `pty_watch.rs:154` / `pty_watch.rs:199` → `watcher::handle_streaming_line_with_session`. That one function calls **both** the changed code and a second, unchanged marker write:

- `src/watcher/stream.rs:112` → `agent.parse_event(...)` → `opencode::parse_json_event` → the new `mark_rate_limited_for_evidence` (`src/agent/opencode.rs:159-165` at 846c14d0).
- `src/watcher/stream.rs:114-134` → for the same line, because `event_kind == EventKind::Error`, `refusal_on_channel(line, …)` then `rate_limit::mark_rate_limited_for_message(&agent.kind(), …, &message)`.

The `message` handed to that second call is **not** the raw line. `refusal_on_channel` (`src/rate_limit.rs:538-551`) runs `provider_attributable`, which pushes *each* envelope string as its own line (`src/quota_channel.rs:215-242`), and then `quota_line` (`src/agent/stream_completion.rs:217-233`) returns only the single line carrying the signature — the provider's sentence, windowed by `enclosing_plain_run`. For the captured refusal that is:

```
Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_01/billing
```

That string contains no `providerid` / `provider_id` / `provider` / `model` substring, so `group_from_refusal` returns `None` (`src/agent/model_group.rs:103-107`) and `mark_rate_limited_for_evidence` falls through to `mark_rate_limited` (`src/rate_limit.rs:117-119`) — the **agent-wide** marker, written microseconds after the provider-scoped one and never removed by it.

Consequence: `is_rate_limited(OpenCode, None)` is still true, and `src/cmd/run_dispatch_resolve.rs:148` (`dispatch_blocking_hold`) still reroutes the whole agent. The measured incident reproduces unchanged, including for an explicitly named working route, because that gate fires before any model-scoped gate.

The two new tests cannot see this: `src/rate_limit_credibility_tests.rs:79-127` call `parse_json_event` directly and never enter `handle_streaming_line_with_session`. They prove a unit behaviour that production immediately overwrites.

**Q1 verdict: PASS on "does a genuinely exhausted provider stay blocked" — but only because the hold is still agent-wide. The commit's stated purpose is not achieved.**

### 2. A provider-scoped hold does not block a dispatch that names no model (HIGH — the failure mode that appears once finding 1 is fixed)

`dispatch_blocking_hold_for_model` (`src/rate_limit.rs:262-273`) short-circuits on `model_group(...)?`. For OpenCode, `model_group` is `model.and_then(provider_from_model)` (`src/agent/model_group.rs:52-55`) — it returns `None` when the model is `None` **or** when the model string has no `provider/` prefix.

At `src/cmd/run_dispatch_resolve.rs:255-259` the value passed is `effective_model`, which is `None` whenever the caller passed no `--model` and `agent_config::get_default_model("opencode")` is unset (`run_dispatch_resolve.rs:195-199`). `healthy_model_for` cannot compensate either: `groups_for_agent(OpenCode)` is empty (`src/agent/model_group.rs:130-146`), so `src/cmd/run_dispatch_resolve.rs:238-253` never switches an opencode route.

Failure scenario: provider `opencode` (Zen) is genuinely out of balance, a scoped marker `~/.aid/rate-limit-opencode--opencode` exists, the agent-wide marker does not. `aid run opencode "…"` with no `-m` → both gates return `None` → the run is dispatched straight into the exhausted provider and burns.

The same gap applies to every other availability consumer, all of which read only the agent-wide marker: `src/cmd/batch_validate.rs:47`, `src/cmd/batch_helpers.rs:81`, `src/cmd/batch_dispatch.rs:87`, `src/agent/selection_scoring.rs:117`, `src/agent/selection_advice.rs:243`, `src/background_reaper.rs:224`, `src/rate_limit_wait.rs:22`, `src/cmd/hook.rs:72`. Only `aid agent quota` (`src/cmd/agent_display.rs:84-97`) and the model gate see group holds.

### 3. Attribution can extract a token that is not a provider — unknown collapsed into a plausible value (MEDIUM)

`named_opencode_provider` / `value_after_key` (`src/agent/model_group.rs:103-121`) do substring scanning over the blob, not JSON key lookup: first occurrence of the key anywhere, then everything after the *next* `:` up to the next `"`, `,`, `}` or whitespace. The result is then `provider_from_model(value).or(Some(value))` — **any** extracted token becomes a group name; nothing validates that it is a provider.

Two concrete traces against the now-much-larger evidence string:

- Envelope with a `model` object and no provider key, e.g. `"model":{"modelID":"opencode-go/x"}` — this exact block shape is captured at `/Users/mingsun/.aid/tasks/t-b4872736/home/.aid/tasks/t-8adbb860/transcript.md` (`"model":{"modelID":"kilo-auto/free","providerID":"kilo"}`). With no provider key present, the `"model"` fallback yields the text after the next `:`, which is `{"modelID"…`; the first delimiter is the `"` at index 1, so the extracted group is the single character `{`.
- Message embedding an upstream JSON body. Captured on this machine for grok: `{"type":"error","message":"Internal error: {… \"modelCalls\": 25, …}"}`. Applied to an opencode-family envelope of that shape, the first `"model"` hit is `modelCalls`, and the extracted group is `25`.

Either writes `~/.aid/rate-limit-opencode--{` or `--25`. `model_group` never returns those, so the marker blocks nothing — and because attribution "succeeded", the agent-wide marker is not written. That is the hold-that-does-not-hold case the brief flags as the worst outcome.

The `"unknown"` guard (`model_group.rs:109`) only catches a literal `unknown`; it does not constrain the value to a plausible provider id.

### 4. The evidence shape the fix depends on has never been captured; on the one shape that has, the fix is a no-op (MEDIUM)

The only opencode refusal in this repo's captured record is, verbatim from `src/rate_limit_signatures.rs:78-89` (captured from t-76181278) and `src/rate_limit_signatures_tests.rs:132`:

```json
{"type":"error","error":{"name":"APIError","data":{"message":"Insufficient balance. Manage your billing here: https://opencode.ai/workspace/wrk_01/billing","statusCode":401}}}
```

No `providerID`, no `provider`, no `model`. `group_from_refusal` returns `None` on it → agent-wide hold → unchanged behaviour.

A grep for `"providerID":"…"` across every task record under `~/.aid/tasks/` returns exactly **one** hit, and it is inside a kilo assistant-message metadata block, not an error event. The new test's top-level `"providerID":"opencode"` is therefore asserted, not observed.

Related ordering hazard: `serde_json = "1"` with no `preserve_order` feature (`Cargo.toml:25`), so `v.to_string()` emits object keys **sorted**, not in wire order. Top-level `"error"` sorts before `"providerID"`, so a `providerID` nested inside the error object — or one embedded in the message text of an upstream body — is found *before* a top-level one. Attribution takes whichever sorts first, which is not necessarily the provider that refused.

**Q2 verdict: FAIL.** The extraction is plausible-shaped, not correct: it is unvalidated substring scanning over a blob whose serialisation order the code does not control, and its only supporting fixture is invented.

### 5. Opencode forks route through the changed code and get nothing from it (LOW)

`src/agent/opencode_overlay.rs:124-131` passes `spec.reported_kind` as `marker_kind` for kilo, mimocode and custom overlays. `group_from_refusal` branches only on `OpenCode` and `Cursor` (`src/agent/model_group.rs:82-88`), and `provider_for_cli` gives Kilo/MiMoCode/Custom `MeteringShape::Unknown` (`src/types/provider.rs:149-151`), so `has_grouped_quota` is false for them. These CLIs are equally multi-provider; the fix is narrower than the class of bug it names. Behaviour unchanged — noted for completeness, not as a regression.

### 6. A group name containing `/` produces an artifact nothing can clear (LOW)

`group_marker_path` (`src/rate_limit.rs:60-68`) joins `aid_dir()` with `rate-limit-<slug>--<group>`. If `group` starts with `/`, `Path::join` re-roots or nests, and `write_marker`'s `create_dir_all(parent)` (`src/rate_limit.rs:196-198`) creates a *directory* named `rate-limit-opencode--`. `discovered_group_markers` (`src/rate_limit.rs:400-416`) then strips the prefix, gets an empty group and filters it out, so `aid config clear-limit opencode` never removes it. Reachable only via a finding-3 style extraction (`provider_from_model` normally strips the `/`; a value beginning with `/` survives via the `.or(Some(value))` fallback).

---

## Per-question verdicts

**Q1 — did this make holds stop holding?** PASS with a caveat that undercuts the commit. A genuinely exhausted provider is still blocked, but only via the agent-wide marker that finding 1 shows is still written by `src/watcher/stream.rs:130`. The `None` fallback is correct (`src/rate_limit.rs:117-119`): unattributable refusals go agent-wide, never to no hold. **No signature widening**: detection still runs on `detail`, not on the evidence (`src/agent/opencode.rs:158`), and `classify_hold` / `write_marker` still consume `message` (`src/rate_limit.rs:113-116`, `196-206`). The larger string reaches only `group_from_refusal`. Findings 2 and 3 are the paths by which a hold *would* stop holding.

**Q2 — is the attribution correct or merely plausible?** FAIL. See findings 3 and 4.

**Q3 — does the marker lifecycle still work?** PASS.
- `aid config clear-limit opencode` → `src/cmd/config.rs:229/244/252` → `clear_all_rate_limits_for_agent` (`src/rate_limit.rs:384-398`), which prefix-scans `~/.aid` via `discovered_group_markers` — OpenCode-specific and not limited to the static `groups_for_agent` table. A `rate-limit-opencode--opencode` marker is found and cleared.
- `write_marker` fields are unchanged and still derived from the human-readable `message`: `insufficient balance` → `QuotaRecovery::NeedsHuman` → `hold: manual`, empty `recovery_at`, plus `provider: <group>` for OpenCode group markers (`src/rate_limit.rs:71-78`, `196-207`).
- Visibility: `active_group_holds` includes discovered markers (`src/rate_limit.rs:663-690`) so `aid agent quota` renders `Partial` (`src/cmd/agent_display.rs:84-97`). `aid config agents` shows nothing for a group-only hold (`src/cmd/config_display.rs:145-147`) — pre-existing for cursor/agy too, not introduced here.
- One pre-existing cross-provider hazard, not from this commit: `live_quota::overrides_marker` maps `AgentKind::OpenCode` to the single aidbar provider `"opencode"` (`src/live_quota.rs:58-69`) regardless of which provider group the marker names, so an aidbar Zen reading could release a hold recorded against a *different* opencode provider. Unreachable for the Zen `NeedsHuman` marker (`live_quota_can_override` returns false), reachable for a transient group hold.

**Construction sites of the changed API.** `mark_rate_limited_for_evidence` is new; its only external caller is `src/agent/opencode.rs:159-165`. `mark_rate_limited_for_message` now delegates to it passing `message` twice (`src/rate_limit.rs:105`), so its four call sites — `src/watcher.rs:285`, `src/cmd/run_lifecycle.rs:271`, `src/cmd/run_lifecycle.rs:543`, `src/watcher/stream.rs:130` — are semantically unchanged. All four were checked; none needed updating.

## Overall: BLOCK

Not because the change is dangerous — it is close to inert — but because shipping it as "OpenCode holds are now scoped to the refusing provider" would state something the pipeline does not do (finding 1) on an event shape we have never captured (finding 4), while the completion of that work would introduce a hold that does not hold (findings 2 and 3). The release note would tell the operator the incident is fixed; a re-run reproduces it.

Minimum to unblock: stop `src/watcher/stream.rs:130` from re-marking agent-wide when the adapter already attributed the same line; make the model-less dispatch gate consult opencode group holds; and constrain the extracted token to a validated provider id instead of `.or(Some(value))`.

## Open Questions

- **Does opencode actually emit `providerID` on an error envelope?** Unknown. Nothing in this repo or in any captured task log shows one. Determining this needs a live opencode run against an exhausted provider, which the read-only red lines exclude.
- **Nothing was executed.** No build, no `cargo test`, no `aid` invocation — per the red lines. Every claim above is a code trace over commit 846c14d0 plus grep over `~/.aid/tasks/`. In particular I did not observe the double-write of finding 1 at runtime; I traced it through `pty_watch.rs:154` → `stream.rs:112` → `stream.rs:130` and the `refusal_on_channel` → `quota_line` chain. The cheapest confirmation is a test that drives `handle_streaming_line_with_session` with the captured envelope and asserts `!is_rate_limited(OpenCode, None)`.
- **Whether `opencode` runs in this repo normally carry an explicit `--model`.** If they always do, finding 2's blast radius is limited to the batch/selection consumers listed rather than to `aid run`; `agent_config.toml` was not read as part of this review.