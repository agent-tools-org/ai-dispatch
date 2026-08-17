# Investigation: Model Discovery

## Problem

FINDING: AID's static agy catalog omitted models served by the installed CLI.
CONFIDENCE: HIGH
EVIDENCE: `src/model_catalog_data.rs:134-136` contains three agy rows; the 2026-08-17 `agy models` capture contains 14 rows, from `gemini-3.7-flash-high` through `gpt-oss-120b-medium`.
IMPLICATION: Static catalog consumers such as agent listing and held-route metadata can lag the CLI.
NEXT: Merge served names without inventing metadata.

## Root Cause

FINDING: The agy CLI probe already existed before this change.
CONFIDENCE: HIGH
EVIDENCE: `src/agent/antigravity.rs:103-110` runs `agy models` through the shared probe runner and parses stdout.
IMPLICATION: No new probing mechanism is required.
NEXT: Inspect the existing cache boundary.

FINDING: The probe already had a 10-second bound and a 24-hour disk cache.
CONFIDENCE: HIGH
EVIDENCE: `src/agent/model_validation.rs:35-36` defines `DEFAULT_PROBE_TIMEOUT` and `SERVED_MODELS_CACHE_TTL`; `src/agent/model_validation.rs:230-264` reads the process/disk cache before probing.
IMPLICATION: A second `OnceLock` would defeat TTL refresh for long-lived processes.
NEXT: Reuse the disk cache directly.

FINDING: The pre-fix catalog query never consulted served-model data.
CONFIDENCE: HIGH
EVIDENCE: `bfe02895:src/model_catalog.rs:186-193` returned only Qwen config rows or filtered `AGENT_MODELS` rows.
IMPLICATION: `aid agent list --json` could not expose newly served agy models even after validation cached them.
NEXT: Wire cached agy names into the resolved catalog.

FINDING: Allowing `models_for_agent` to initiate a network probe is not safe for every caller.
CONFIDENCE: HIGH
EVIDENCE: The cold 2026-08-17 `agy models` capture took from 21:12:53 to 21:15:18; dispatch route resolution reads `models_for_agent` at `src/cmd/run_dispatch_resolve_held.rs:101-106`.
IMPLICATION: Dispatch-time catalog reads must remain cache-only; the existing validation probe remains separately bounded.
NEXT: Warm discovery only from the explicit agent-list inspection path.

## Fix

FINDING: Agent listing warms the existing cache, while resolved catalog reads remain disk-only.
CONFIDENCE: HIGH
EVIDENCE: `src/cmd/agent_json.rs:87-89` invokes the existing cached probe for installed agy; `src/model_catalog_resolved.rs:77-105` merges only fresh disk-cached names.
IMPLICATION: `aid agent list --json` discovers agy models without adding network work to dispatch catalog callers.
NEXT: Preserve unknown metadata explicitly.

FINDING: Discovered pricing and capability are represented as unknown, not zero.
CONFIDENCE: HIGH
EVIDENCE: `src/model_catalog_resolved.rs:97-105` assigns `None` to both prices and capability; `src/cost/pricing_resolution.rs:9-12` refuses similar-name pricing for non-static agy models.
IMPLICATION: JSON emits `null` and cost display emits `unknown`; discovered models cannot masquerade as free or lowest-capability.
NEXT: Verify the CLI flows and mutation-sensitive fixture test.

FINDING: The captured fixture is mutation-sensitive and the affected automated paths pass.
CONFIDENCE: HIGH
EVIDENCE: Mutating `gemini-3.7-flash-high` to `gemini-3.7-flash-hugh` made `parses_captured_agy_models_output_exactly` fail with a left/right list mismatch; after restoration, the parser test, direct catalog-merge test, advise unknown-capability scoring test, two `discovered_agy` tests, 14 model-validation tests, guide coverage, and `init_e2e` passed.
IMPLICATION: The parser, cache merge, JSON nulls, validation, cost unknown, and official guide contract are covered.
NEXT: Run the rebuilt binary acceptance commands when the mandated shared target is writable.

SUMMARY: 8 findings, 8 high-confidence
