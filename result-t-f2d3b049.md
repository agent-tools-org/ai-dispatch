## Findings

1. **Fixed — OpenCode streamed refusals could still create an agent-wide hold.** `src/agent/opencode.rs:156-165` previously marked the route while parsing an event, before the watcher knew the dispatched model. Real insufficient-balance JSON has no `providerID`, so that path wrote `rate-limit-opencode`. OpenCode built-in attribution now happens at `src/watcher/stream.rs:123-171`, where the task's requested model is available.

2. **Fixed — refusal writers disagreed about attribution.** The per-line stream path, stderr post-processing (`src/watcher.rs:271-303`), and lifecycle fallback paths (`src/cmd/run_lifecycle.rs:271-281`, `:548-558`) used message-only marking. They now use `mark_rate_limited_for_model` (`src/rate_limit.rs:109-149`), which prefers the dispatched route, keeps the refusing provider held, and falls back conservatively when no route is known. Completion-path marking uses the same helper.

3. **Fixed — parsed-event attribution was vulnerable to textual coincidence.** Parsed JSON values now use exact key lookup only when no dispatched model is available (`src/rate_limit.rs:134-149`). The regression fixture includes the real event shape with no `providerID`; a separate test proves an error message mentioning `provider` before a real top-level key still attributes to the real key. Provider marker paths are normalized case-insensitively.

4. **v10.26.0 coverage gap identified.** That release added provider-derived groups, provider metadata in markers, discovered-group clearing, and pre-dispatch enforcement for an already-known held model route. It did not update the OpenCode adapter's streamed error path, watcher per-line/stderr writers, or lifecycle fallback writers to use the dispatched route. Those sibling paths are what reproduced this incident.

## Verification

- `aid build`: passed; only pre-existing warnings.
- `aid build clippy`: passed; only existing warnings.
- OpenCode-focused suite: 28 passed, 0 failed.
- Provider-attribution filter: 6 passed, 0 failed.
- Decisive watcher regression: passed after the fix.
- Mutation check: removing route precedence caused `streamed_opencode_refusal_holds_only_the_dispatched_provider` to fail at `assertion failed: !crate::rate_limit::is_rate_limited(&AgentKind::OpenCode, None)`.
- Full suite: 2340 passed, 18 failed, 9 ignored. The observed isolated failure is the unrelated nested-dispatch-depth test at `src/cmd/batch_tests/helpers.rs:221`.
