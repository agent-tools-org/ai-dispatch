# Gemini auto-model implementation — audit

## Findings

- **Low — Spec vs matcher for Gemini 3 “pro”.** The task requested `m.contains("gemini-3") && m.contains("pro")`. Matching `preview` accidentally satisfies `contains("pro")`. Implementation uses **`m.contains("gemini-3") && m.contains("-pro")`** so names like `gemini-3.1-pro-preview` match while **`gemini-3-flash-preview` does not** (see [`src/cost/pricing_builtin.rs`](src/cost/pricing_builtin.rs)). Same rates as specified.

- **Informational — `estimate_cost(_, None, Gemini)` cache interaction.** Fallback uses `GEMINI_DEFAULT_MODEL_CACHE`; if warmed from the DB, default cost tracks the latest successful Gemini task model; otherwise **`gemini-3-flash-preview`** ([`src/cost/mod.rs`](src/cost/mod.rs), [`src/main.rs`](src/main.rs)).

- **Informational — `aid config agents` pricing refresh.** When not built under `cfg(test)`, if `AID_NO_PRICING_REFRESH` is unset and **`pricing.json` is missing or older than 24h**, a detached **`curl -fsSL -o … -z …`** run is spawned (non-blocking) ([`src/cmd/config.rs`](src/cmd/config.rs)).

## Open Questions

- None.

## Scope verification

| Area | Evidence |
|------|----------|
| AGENT_MODELS Gemini aliases + `gemini-3.*` + legacy `gemini-2.5-*` | [`src/cmd/config_models.rs`](src/cmd/config_models.rs) |
| `Store::latest_default_model`, warm on startup | [`src/store/queries/task_queries.rs`](src/store/queries/task_queries.rs), [`src/main.rs`](src/main.rs) |
| Config “Recent:” undeclared model counts | [`src/cmd/config_display.rs`](src/cmd/config_display.rs) |
| Tests: cost Gemini 3 previews, merged catalog, recent line, DB latest model | [`src/cost/mod.rs`](src/cost/mod.rs) (`#[cfg(test)]`), [`src/cmd/config_tests.rs`](src/cmd/config_tests.rs), [`src/store/tests/task_tests.rs`](src/store/tests/task_tests.rs) |
| File size cap | Builtin matcher split → [`src/cost/pricing_builtin.rs`](src/cost/pricing_builtin.rs) (~128 lines); [`src/cost/mod.rs`](src/cost/mod.rs) (~247 lines) |

## Verify

`cargo check` and targeted `cargo test cost::tests`, `cargo test config::tests`, `cargo test latest_default_model_prefers` completed successfully after changes.
