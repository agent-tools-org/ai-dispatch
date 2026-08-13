[MILESTONE] Audited touched files for provenance correctness
[MILESTONE] Discovered batch_retry.rs degradation failure
[MILESTONE] Analyzed retry.rs conservative fallback for old tasks
[MILESTONE] Searched untouched CLI entrypoints and found omission in ask.rs and others
[MILESTONE] Found remaining is_none() semantic inference in lifecycle files
[MILESTONE] Compiled final findings

## Findings

**1. `src/cmd/batch_retry.rs` — FAIL (High Severity)**
* **Evidence:** `src/cmd/batch_retry.rs` adds `model_source: ModelSource::AidResolved` hardcoded when copying `task.requested_model`.
* **Detail:** A user's explicit `--model` supplied in the original batch run gets stored in the database's effective model (`task.requested_model`). When `batch_retry.rs` recreates `RunArgs` from the task, it forces the source to `AidResolved`. 
* **Impact:** A caller's explicit model choice will degrade silently on retry instead of hard-erroring if the model becomes stale or unsupported.

**2. Untouched CLI Entrypoints — FAIL (Medium Severity)**
* **Evidence:** `src/cmd/ask.rs` (line 111), `src/cmd/explain.rs` (line 37), `src/cmd/mcp_tools.rs` (line 21), and `src/cmd/experiment.rs` (line 21) construct `RunArgs` using `..Default::default()` without setting `model_source`.
* **Detail:** Because `ModelSource::default()` is `UserSupplied`, if the user omits a model, `model` is `None` but `model_source` defaults to `UserSupplied`. `run_dispatch_prepare.rs` then clones this and sets the effective model without updating the source. 
* **Impact:** Aid-chosen models in these paths are permanently saved into `dispatch_args.json` as `UserSupplied`. The original bug survives: these tasks will hard-error on retry if the aid-chosen model is removed. They needed touching.

**3. Semantic Inference via `is_none()` — FAIL (Low/Medium Severity)**
* **Evidence:** `src/cmd/run_verify.rs`, `src/cmd/run_dirty.rs`, `src/cmd/run_post.rs`, and `src/cmd/run_iterate.rs` all use the construct:
  ```rust
  if retry_args.model.is_none() { 
      retry_args.model = task.requested_model.clone(); 
      retry_args.model_source = crate::agent::model_validation::ModelSource::AidResolved; 
  }
  ```
* **Detail:** These sites are explicitly using `retry_args.model.is_none()` to infer that the model source MUST be `AidResolved`. This subverts the whole point of `model_source`. The `retry_args` clone *already carries* the correct `model_source` (populated by `run_batch_args.rs`). Overwriting it based on `is_none()` is deciding semantics rather than just reading the existing enum value.

**4. `src/cmd/retry.rs` — PASS**
* **Evidence:** `src/cmd/retry.rs` uses `RunArgs::saved_for_task(store, task.id.as_str())?.unwrap_or_else(|| ... )` with a fallback setting `model_source: ModelSource::AidResolved`.
* **Detail:** New tasks correctly restore explicit provenance from JSON. The manual fallback only triggers for old tasks without JSON records.
* **Impact:** Treating unknown provenance as caller-supplied (`UserSupplied`) would strand old tasks with stale aid-chosen models in the exact bug being fixed. The developer's documented choice to use `AidResolved` is the correct conservative fallback to let stale models degrade gracefully.

**5. `src/cmd/batch_args.rs` — PASS**
* **Evidence:** Extracts intent directly from the batch TOML using `if task.model.is_some() { ModelSource::UserSupplied } else { ModelSource::AidResolved }`.
* **Detail:** The value landing in `RunArgs` correctly matches the caller's explicit intent at the outer boundary.

**6. `src/background_lifecycle.rs` — PASS**
* **Evidence:** Restores the exact saved provenance using `RunArgs::saved_for_task()`.

## Overall Verdict
**BLOCK**

The fix fundamentally works for the primary `aid run` flow, but `batch_retry.rs` introduces a regression where explicit choices silently degrade, multiple CLI entrypoints leak the original bug by defaulting to `UserSupplied`, and the lifecycle scripts still infer semantics via `.is_none()` instead of honoring the tracked provenance.

=== AID TASK t-0213aaff DONE (exit 0) ===
