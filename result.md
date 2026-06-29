## Findings

### FIX: Model self-heal control flow is not covered by regression tests

The new safety-critical path has no tests exercising the one-shot retry guard, event-chain guard, or force-default resolver behavior. The branch adds `maybe_auto_retry_after_model_unavailable` at `src/cmd/run_post.rs:77`, sets `force_default_model` and clears model/budget fields at `src/cmd/run_post.rs:105`, and bypasses model selection in `src/cmd/run_dispatch_resolve.rs:181`; however, the only new tests found are classifier tests in `src/model_health.rs:66`. `rg` found no test references for `force_default_model`, `model_self_healed`, or `maybe_auto_retry_after_model_unavailable`. This violates the project rule that all changes must have tests and leaves the critical infinite-loop guarantee unpinned.

### FIX: `src/cmd/run_post.rs` exceeds the 300-line file limit

The file is now 351 lines (`wc -l src/cmd/run_post.rs`) after adding the self-heal helpers at `src/cmd/run_post.rs:77`. The project file-size limit is 300 lines. There are broader pre-existing size violations in the repo, but this branch adds to `run_post.rs` and pushes/keeps it above the acceptance threshold.

## Checklist Results

1. **Infinite-loop safety: PASS, test gap noted.** `maybe_auto_retry_after_model_unavailable` returns `None` when `args.force_default_model` is true or any retry-chain task has a `model_self_healed` event (`src/cmd/run_post.rs:88`). The retry sets `force_default_model = true`, clears `model`, and parents the retry to the failed task (`src/cmd/run_post.rs:105`). The self-heal event is inserted before retry dispatch (`src/cmd/run_post.rs:124` then `src/cmd/run_post.rs:125`). `get_retry_chain` walks parent links and includes root-to-current tasks (`src/store/queries/task_queries.rs:68`), so a second failed retry can see the original event if the force flag were lost.

2. **`force_default_model` correctness: PASS.** Forced retries set `requested_model = None` (`src/cmd/run_dispatch_resolve.rs:181`), force `budget_active = false` (`src/cmd/run_dispatch_resolve.rs:186`), skip smart routing (`src/cmd/run_dispatch_resolve.rs:188`), and therefore return `effective_model = None` (`src/cmd/run_dispatch_resolve.rs:207`, `src/cmd/run_dispatch_resolve.rs:233`). This covers explicit `args.model`, configured defaults, smart-route, and budget paths.

3. **Classifier false positives: PASS with residual risk.** The listed transient/auth/billing strings do not match the classifier patterns in `src/model_health.rs:10`: balance, auth, rate limit, `429`, quota, and connection reset are excluded by the current tests at `src/model_health.rs:78`. The loose `model` + `is not available` pattern at `src/model_health.rs:19` could still classify some provider/service-outage wording as model-unavailable, but the retry is bounded to one attempt by the guards above.

4. **Classifier false negatives: PASS.** The classifier covers opencode `Model not found: glm-4.7/.`, mimo `Not supported model ...`, and codex `model is not supported when using Codex` in tests at `src/model_health.rs:66`. JSON extraction handles `{error:{message}}` and `{error:{data:{message}}}` at `src/model_health.rs:47`, with tests for opencode and codex JSON at `src/model_health.rs:86`.

5. **Hook ordering and cleanup: PASS.** The self-heal retry runs after failed-task post-processing, failure hooks, failed-worktree cleanup, completion hooks, and webhooks (`src/cmd/run_lifecycle.rs:72`, `src/cmd/run_lifecycle.rs:145`, `src/cmd/run_lifecycle.rs:155`), then after hang retry and before generic retry/cascade/rate-limit fallback (`src/cmd/run_lifecycle.rs:200`, `src/cmd/run_lifecycle.rs:203`, `src/cmd/run_lifecycle.rs:208`). If a failure is both rate-limited and model-unavailable, failed post-processing marks the rate limit first (`src/cmd/run_lifecycle.rs:448`), but model self-heal wins precedence by returning early at `src/cmd/run_lifecycle.rs:203`.

6. **Model table and pricing validity: PASS.** Codex table entries are `gpt-5.5`, `gpt-5.4`, and `gpt-5.4-mini` (`src/cmd/config_models.rs:173`), and OpenCode entries are the new `opencode/...` IDs (`src/cmd/config_models.rs:185`). `budget_model()` prefers the cheapest non-free model when any non-free model exists (`src/cmd/config_display.rs:224`), so Codex resolves to `gpt-5.4-mini` and OpenCode resolves to `opencode/glm-5.2`. Built-in pricing still covers `gpt-5.4` via GPT-5 matching and `glm-5`/`kimi-k2` via the OpenCode branch (`src/cost/pricing_builtin.rs:115`).

7. **Rules and static checks: FAIL.** `cargo check -p ai-dispatch` passes. `git diff --check main...HEAD` passes. No `features = ["full"]` were introduced. No `unwrap()` calls were found in the changed source files. New `src/model_health.rs` has the required header. Acceptance still fails because `src/cmd/run_post.rs` is 351 lines and the new self-heal/force-default control flow lacks tests.

## Overall

FIX
