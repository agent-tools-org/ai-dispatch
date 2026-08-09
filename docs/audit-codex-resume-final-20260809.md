[MILESTONE] Analyzed Git diff for codex-resume-durable-home branch
[MILESTONE] Traced execution paths for foreground and background codex tasks
[MILESTONE] Evaluated Q1: Found background containerized retries are broken due to missing sandbox override
[MILESTONE] Evaluated Q2: Confirmed sandbox field overload risks future sandboxing logic applying to container runs
[MILESTONE] Evaluated Q3: Traced rollout format failure leading to silent resume drops and lost attribution metrics
[MILESTONE] Drafted final audit report with PASS/FAIL and evidence for all questions
 
1. **Does this branch break anything that already worked?**
- Normal foreground run with no session ID: **PASS**. It correctly injects the real `CODEX_HOME` via `apply_codex_home_env` and starts a fresh session, enabling durable state.
- Retry that resumes a live session: **PASS**. It now checks the host's durable home first. If missing, it falls back to a fresh session (preventing a crash).
- `--sandbox` run: **PASS**. `opts.sandbox` is already true, skipping the host rollout precheck.
- `--container` run: **PASS**. `opts.sandbox` is mutated to true, skipping the host rollout precheck.
- Background run: **FAIL**. The `opts.sandbox = true` override was only added to `run_dispatch_execute.rs` (foreground tasks). In `src/background.rs:178`, `build_command` is called without this override. If a background run is containerized and is a retry (`session_id` present), `opts.sandbox` is false. The code evaluates `!resume_fallback_needed()`, which looks in the *host's* durable home. Since the run is containerized, the rollout won't exist there, and the `session_id` is silently dropped, breaking the retry.

2. **Is the `opts.sandbox` overload safe and can it mislead future changes?**
**FAIL**
I confirm that currently `codex.rs` reads `opts.sandbox` in exactly one place (the `resume_session_id` filter) and no other agent reads it. Behaviorally, it is safe today. However, overloading `sandbox` to mean "skip host rollout checks because we're isolated" is highly misleading. If a future developer adds actual sandboxing logic (e.g., restricting network access, changing file permissions, or applying seccomp filters) inside `apply_run_env` or `build_command` based on `opts.sandbox`, all `--container` runs will suddenly and unexpectedly inherit those restrictions. The intent should be modeled explicitly (e.g., passing `uses_durable_home` to `build_command`).

3. **What happens if Codex writes a rollout whose name does not fit the timestamp shape?**
**FAIL**
If the rollout filename format changes (e.g., `Z` for UTC or seconds omitted), `NaiveDateTime::parse_from_str` will fail. `rollout_filename_matches` will return false.
Consequences:
1. `resume_fallback_needed` will return true. The `session_id` will be silently dropped during `build_command`. The user will see their retries always starting from scratch, wasting time and tokens, with only a `Codex session resume skipped` milestone event.
2. `codex_attribution::session_file_matches` now delegates to this strictly-parsed function. It will fail to find the session file, meaning the user will silently lose token attribution and billing metrics for that run.
This consequence is unacceptable; it tightly couples `aid` to the internal timestamp format of Codex when a simple prefix/suffix match on the UUID was sufficient.

**BLOCK**

What did I miss?

=== AID TASK t-1ff94910 DONE (exit 0) ===
