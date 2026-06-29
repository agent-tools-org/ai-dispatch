## Findings

### 1. FAIL: Pricing lookup now zero-costs every model name containing `mimo`

Evidence:
- `src/cost/pricing_builtin.rs:104-109` returns zero pricing when `m.contains("mimo")`, independent of agent and whether the model is marked free.
- `src/cost/mod.rs:141-145` calls this substring lookup for any agent after overrides miss.
- `src/cmd/config_models.rs:188` already has explicit free OpenCode model `mimo-v2-flash-free`; the broad match also zero-costs names like `xiaomi/mimo-v2.5-pro`.

Impact: Cost accounting can underreport non-free MiMo-family model usage outside MiMoCode.

### 2. FAIL: MiMoCode session continuation is not propagated on retry paths

Evidence:
- `src/agent/mimocode.rs:43-47` supports `--session`, `--continue`, and `--fork`.
- `src/agent/mimocode.rs:73-74` reuses OpenCode JSON parsing, which persists `sessionID` via `src/agent/opencode.rs:178-187`.
- Retry propagation is OpenCode-only at `src/cmd/retry.rs:50-55`, `src/cmd/run_post.rs:64-66`, `src/cmd/run_dirty.rs:164-167`, `src/cmd/run_iterate.rs:143-145`, `src/cmd/run_verify.rs:208-210`, and `src/cmd/run_verify.rs:254-256`.

Impact: MiMoCode retries lose session continuity. Kilo has the same gap, but MiMoCode explicitly wires session flags.

## Checklist

1. FAIL - Exhaustive registration: Core registration is present, but retry session propagation is incomplete. `AgentKind::MiMoCode` is in `src/types/agent.rs:27-56`, parses/displays at `src/types/agent.rs:67` and `src/types/agent.rs:87`, resolves at `src/agent/mod.rs:191-207`, and gets auto `--dir .` at `src/cmd/run_dispatch_resolve.rs:110-129`.

2. PASS - Model default correctness: `src/agent/mimocode.rs:54-55` always appends `-m`, defaulting to valid `mimo/mimo-auto`. Explicit, smart-routing, complex-no-model, and budget paths all resolve safely via `src/cmd/run_dispatch_resolve.rs:178-210`, `src/cmd/config_models.rs:192`, and `src/cmd/config_display.rs:219-227`.

3. PASS - Binary vs agent name: Dispatch name is `mimocode` at `src/types/agent.rs:67` and `src/types/agent.rs:87`; binary detection probes `mimo` at `src/agent/mod.rs:101` and `src/agent/mod.rs:174`; command launch uses `mimo` at `src/agent/mimocode.rs:38`.

4. PASS - Streaming/PTY: MiMoCode streams at `src/agent/mimocode.rs:18-20` and uses default non-PTY behavior from `src/agent/mod.rs:57-61`, matching the stated JSONL stdout behavior.

5. PASS - Error-as-done consistency: `src/agent/mimocode.rs:94-99` matches Kilo and OpenCode behavior at `src/agent/kilo.rs:92-97` and `src/agent/opencode.rs:99-103`.

6. FAIL - Pricing: See finding 1.

7. PASS - Cross-auditor bar: MiMoCode was not added as a default judge or peer reviewer. `--judge` defaults to `gemini` at `src/cli/command_args_a.rs:51-52`; `--peer-review` has no default at `src/cli/command_args_a.rs:53-54`.

8. PASS - File size/rules: `src/agent/mimocode.rs` is 107 lines; `src/agent/mimocode/tests.rs` is 121 lines; reviewed files are under 300 lines. Production reviewed files have no `unwrap()` matches.

## Overall

FIX