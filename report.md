## Findings

- **High — OpenCode provider holds were inert during dispatch.** The resolver now derives the provider from `provider/model`, checks `rate-limit-opencode--<provider>`, and cascades before dispatch (`src/rate_limit.rs:262`, `src/cmd/run_dispatch_resolve.rs:234`).

- **Medium — `unknown` is not treated as attribution.** Such refusals now create an agent-wide marker with `provider: unknown` (`src/agent/model_group.rs:103`).

- **Low — Ollama is not special-cased.** A hypothetical Ollama refusal narrows only Ollama and cannot disable unrelated providers.

Markers retain the complete refusal message and usable ISO reset timestamps. Missing reset information remains unknown.

## Verification

- Before fix: dispatch test failed with `left: OpenCode`, `right: Codex`.
- After fix: 5 credibility tests passed.
- Full suite: 2,299 passed, 0 failed, 9 ignored.
- `aid build`: 0 errors.
- Commit: `798f4e11`.