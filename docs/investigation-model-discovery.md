# Investigation: Model Discovery

## Problem
`aid` must learn an agent CLI's real model list from the CLI itself, instead of relying solely on its hardcoded table. Currently, the static catalog rots silently for every agent. For example, `aid agent list --json` reports only 3 hardcoded models for `agy`, whereas `agy models` returns 14. This causes `aid` to drop newly discovered models (like `gemini-3.7-flash-high`) because they are unknown.

## Evidence
- `agy models` returns 14 models today, including the `gemini-3.7-flash` family.
- `src/model_catalog_data.rs:134-136` hardcodes exactly three Antigravity models (`gemini-3.6-flash-medium`, `gemini-3.1-pro-high`, `claude-sonnet-4-6`).
- As a consequence, `aid agent list --json` only reports the three hardcoded models, hiding the actual capability of the underlying agent.

## Root Cause
1. `models_for_agent` in `src/model_catalog.rs` filtered and returned models exclusively from the `AGENT_MODELS` static catalog without augmenting it with models discovered via live CLI probing.
2. `get_served_models_cached_with_status` blocked synchronously (up to 10 seconds timeout) to run `agy models` if the cache was cold. Calling this synchronously during dispatch or from `models_for_agent` would slow down dispatch execution.
3. Selection logic (`model_on_budget_preference`, `pick_in_tier`, `model_for_task_budget`) queried `AGENT_MODELS` directly instead of calling `models_for_agent(&kind)`.

## Fix
1. **Never slow dispatch**: Replaced synchronous cache-miss blocking with `get_served_models_fast` in `validate_model_for_agent` (`src/agent/model_validation.rs`). If the cache is cold, it spawns a background thread to refresh the cache and returns `None` immediately, allowing dispatch to proceed instantly and use the models once cached.
2. **Merge Discovered Models**: Updated `models_for_agent` in `src/model_catalog.rs` to fetch discovered `agy` models using the cached live probe, format them as explicitly `unknown` tier/capability (cost $0.00), leak them into static references (`Box::leak`), and merge them with the static `AGENT_MODELS` list.
3. **Seamless Integration**: Updated selection functions (`model_on_budget_preference`, `pick_in_tier`, `model_for_task_budget`) to use `models_for_agent(&kind)` instead of hardcoding a filter on `AGENT_MODELS`, bringing newly discovered models into `--model`, `aid advise`, and `aid agent list` correctly.
4. **Test Fixture Validation**: Added a test `parse_agy_models_output_from_real_output` using actual output from `agy models`. The test fixture was intentionally mutated to prove failure, then corrected to ensure the parsing logic correctly identifies all 14 models without swallowing valid output.
