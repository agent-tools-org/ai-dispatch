# MiMo Code CLI Agent Integration — Plan

Status: **SHIPPED** (v8.101.0, 2026-06-29) — adapter + registration landed and cross-audited.
Owner: dev-manager (老张) orchestrated; implemented via `aid run codex` on `feat/mimocode-agent`.

## Implementation notes (resolved at build time)

- Binary: `/Users/mingsun/.mimocode/bin/mimo` (binary name `mimo`, v0.1.3). Agent dispatch name is `mimocode`.
- `mimo run` **has `--dir`** → adapter mirrors `kilo.rs` (`--dir` + `current_dir`).
- **Default model MUST be `mimo/mimo-auto`.** MiMo's own CLI default (`mimo-v2.5-pro-ultraspeed`) is
  rejected by the server (HTTP 400). The adapter injects `mimo/mimo-auto` whenever no model is set, so
  complex/non-routed dispatches no longer fail in ~1s. (Caught by live smoke test, not the original plan.)
- Streams opencode-shaped JSONL on stdout without a PTY; reuses `opencode::parse_json_event`.
- Pricing zero-cost is scoped to the native `mimo/` provider (not a broad `contains("mimo")`).

## Phase 2 / deferred follow-ups

- **Retry session continuity.** `build_command` wires `--session`/`--continue`/`--fork`, but the retry/iterate
  plumbing (`run_post.rs`, `run_dirty.rs`, `run_iterate.rs`, `run_verify.rs`, `retry.rs`) propagates `session_id`
  for **OpenCode only**. MiMoCode retries therefore start fresh sessions. **Kilo has the identical gap** — fix
  both together by generalizing the OpenCode-only session-propagation checks to the opencode-family agents.

---

## Original plan (for reference)

## Goal

Add Xiaomi's **MiMo Code CLI** as a first-class aid agent (`mimocode`), alongside codex / gemini / cursor /
opencode / kilo. Dispatchable via `aid run mimocode "<prompt>"`.

## Key finding — MiMo Code is an opencode-architecture fork

The non-interactive surface maps almost 1:1 onto the opencode adapter aid already ships. This makes the
integration small (≈ the size of `src/agent/kilo.rs`, which already reuses opencode's parser).

Source: https://mimo.xiaomi.com/zh/mimocode/cli-options (fetched 2026-06-29).

| Capability | MiMo Code | opencode (aid `src/agent/opencode.rs`) |
|---|---|---|
| One-shot run | `mimo run "<msg>"` | `opencode run "<msg>"` |
| JSON event stream | `--format json` (`default` or `json` raw JSON events) | `--format json` |
| Model select | `-m` / `--model` (`provider/model`) | `-m provider/model` |
| Agent select | `--agent` | n/a |
| Auto-approve perms | `--dangerously-skip-permissions` | (similar) |
| Session continue | `--session`/`-s`, `--continue`/`-c`, `--fork` | session id handling |
| Attach to server | `--attach <url>`, `--port` | n/a (mimo also has `mimo serve`) |
| Attach a file | `--file`/`-f` | n/a |

Other commands: `mimo` (TUI w/ `[project]` arg, `--prompt`, `--model`, `--agent`, `--port`, `--hostname`),
`mimo attach [url] --dir -s`, `mimo models [provider] --refresh --verbose`, `mimo github run --event --token`,
`mimo serve --port`.

Because `mimo run --format json` emits the same opencode-style JSONL event stream, the new agent should
**reuse `opencode::{parse_json_event, classify_text_line, extract_tokens_from_output}`** exactly as
`src/agent/kilo.rs` does (see `kilo.rs:7`). Do NOT write a new parser from scratch.

## ⚠️ Naming caveat (do not conflate)

"mimo" already means something in aid: `opencode/mimo-v2-flash[-free]` is a **model** served via opencode, and
the **Cross-Audit Protocol explicitly bars mimo as a sole reviewer** (it misses unit-mismatch / sustained-
runtime / rate-budget bugs). That model and this **MiMo Code CLI agent** are different things (both Xiaomi).

Decisions:
- Name the agent **`mimocode`** (not `mimo`) to avoid collision with the model id and the audit warning.
- Keep the audit-protocol constraint: do NOT add `mimocode` to the trusted cross-auditor set until its
  output quality is empirically validated. Treat it like opencode-tier for dispatch, not for audit.

## Implementation checklist

1. `src/agent/mimocode.rs` implementing the `Agent` trait, modeled on `kilo.rs`/`opencode.rs`:
   - `build_command(prompt, opts)`: `mimo run "<prompt>" --format json -m <provider/model>
     --dangerously-skip-permissions`; set the `Command` **cwd** to the worktree/effective dir (see Open
     Question 2 — mimo `run` has no `--dir` flag; working dir is almost certainly process cwd).
   - `parse_event`: delegate to `opencode::parse_json_event` (same event shape).
   - `streaming() -> true`, `needs_pty() -> true` (match opencode).
   - token extraction via `opencode::extract_tokens_from_output`.
   - Map aid steering/retry onto `--continue`/`--session`/`--fork` if/when needed (phase 2; not required for
     the first dispatch-only cut).
2. Register in the `AgentKind` enum + `src/agent/registry.rs` (parse_str, display name, binary detection —
   probe for the `mimo` binary on PATH).
3. Model defaults in `~/.aid/agent_config.toml` handling + `aid agent config mimocode --model ...`.
4. Tests modeled on `src/agent/kilo.rs` tests (parse a captured `mimo run --format json` event line into the
   expected `TaskEvent`s; build_command argument assertions).
5. Live integration test against the locally-installed mimo (see Open Question 1 for the binary path).
6. Docs: add `mimocode` to README/CLAUDE.md agent table + the `aid run` agent-selection guidance.

## Open questions to resolve at implementation start

1. **Binary path / invocation.** mimo IS installed locally (data dir `~/.local/share/mimocode/` with auth key
   `mimo-code-cli-key-8bddd89c`), but the executable was NOT on the non-interactive shell PATH this session
   (`which mimo` failed; not in bun/npm global that we found). FIRST STEP next session: get the real path +
   help text from the user's interactive shell:
   `! which mimo; mimo --version; mimo run --help 2>&1 | head -30`
   Confirm exact flag spellings (`--format json`, `--model`/`-m`, `--dangerously-skip-permissions`) and how
   `mimo run` sets the working directory (cwd vs a project arg).
2. **Working directory.** `mimo run`'s flag table has no `--dir` (only `mimo attach` does). Plan assumes the
   adapter sets the spawned `Command`'s cwd to the worktree. Verify with `mimo run --help`.
3. **Auth/env.** Auth uses mimo's own key store (`~/.local/share/mimocode/`), so aid likely needs no API-key
   plumbing — confirm via https://mimo.xiaomi.com/zh/mimocode/env-vars (page did not extract cleanly via
   ai-summary on 2026-06-29; read it directly or `mimo models --verbose`). Note any `MIMO_*` env vars that
   gate model/provider so `--budget`/model routing can target them.
4. **Streaming/PTY.** Confirm mimo `run --format json` streams JSONL on stdout without a TTY (opencode needs a
   PTY — `needs_pty() == true`). If mimo behaves the same, mirror opencode; if it streams fine without a PTY,
   prefer the non-PTY path.

## Why this is a good opportunity

- Low cost: reuses the opencode event parser; net new code ≈ kilo.rs (~110 lines + tests).
- Adds a locally-authenticated, no-API-key agent to the roster (the key store is already set up).
- Validates aid's agent-abstraction extensibility (a clean 3rd opencode-family adapter after kilo).
