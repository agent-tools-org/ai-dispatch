# CLI adapter audit — opencode family (`opencode`, `kilo`, `mimocode`)

**Date:** 2026-08-05  
**Status:** read-only audit (no fixes)  
**Evidence root:** `/tmp/aid-wg-wg-e3822c9f/opencode-family-audit/`  
**Installed versions (captured):** opencode `1.18.1`, kilo `7.0.47`, mimo (`mimocode`) `0.1.3`

## Family builder fact

These three share OpenCode-compatible JSONL streaming. Wiring on current tree:

| Agent | Binary | Builder | Extra flags aid passes |
|---|---|---|---|
| `opencode` | `opencode` | `src/agent/opencode.rs` (`OpenCodeAgent`) | `--format json --thinking` (+ optional session/variant/model/dir/`-f`); env `OPENCODE_CONFIG_CONTENT` allows `external_directory` |
| `kilo` | `kilo` | `opencode_overlay` via `src/agent/kilo.rs` | same + **`--auto`**; no `OPENCODE_CONFIG_CONTENT` |
| `mimocode` | **`mimo`** (not `mimocode`) | `opencode_overlay` via `src/agent/mimocode.rs` | same + **`--dangerously-skip-permissions`**; forces `-m mimo/mimo-auto` |

Overlay and native opencode builders are siblings, not one shared function — but they emit the same flag shape aside from the permission flag / default model differences above.

---

## Stall investigation (highest value)

### Historical evidence

| Task | Duration | What the stream did |
|---|---|---|
| `t-99cfb89a` (opencode, 2026-07-29) | **42m 07s**, exit 1, 0 files changed | ~5.4 min of real JSON (`step_*` / `tool_use`), last event **`step_start` with no `step_finish`**, then **~36 min** of only aid idle-nudge plaintext (`Task appears idle. Status update please?`) repeated in the log. No `hung_detected`. |
| `t-61567155` (opencode, today) | ~14 min then **user kill** | Work until 17:16:11, then idle warn / auto-nudge cycles (17:19–17:26). Aid recorded `Replied` / `Acked reply` for nudges; **no further agent JSON**. User killed at 17:29. No `hung_detected`. |
| `t-a6654495` (opencode cascade today) | 0 ms | Failed in **worktree setup** before CLI start — not a stall. |

JSON activity span for `t-99cfb89a` (from log timestamps): **322190 ms** of CLI events, then silence until FAIL.

### Live reproduction (permissions)

Under a real PTY (`script`):

- **Without `--auto`**, bash writing under `/tmp/*`:  
  `permission requested: external_directory (/tmp/*); auto-rejecting`  
  then `tool_use` error `The user rejected permission to use this specific tool call.`  
  Marker file **not** created. (`pty-bash-noauto.txt`)
- **With `--auto`**: bash completed, marker created (`AUTOOK`). (`pty-bash-auto.txt`)

Aid’s opencode adapter **does not pass `--auto`** (kilo does; mimocode passes the mimo-named skip-permissions flag instead). It only injects `OPENCODE_CONFIG_CONTENT` for `external_directory=allow`. Live aid-like env still failed an external `/tmp` bash with `Error: Unexpected error` (`aidlike-bash.raw`).

In-dir write/bash without `--auto` often succeeds (permissions already allowed for the project dir) — so many tasks appear “fine” until they need a gated permission or the model hangs mid-`step_start`.

### Idle watchdog vs stall

Captured tasks show **idle warn + auto-nudge for far longer than 600s** without `hung_detected`, while nudge text is appended into the agent log and aid marks nudges as acked. Whatever the CLI was waiting on (model stream after `step_start`, or a permission that never completed), **nudges did not resume JSON progress** and **did not kill the task**. That matches the “50 minutes / zero file changes / ends on idle nudge” report shape.

---

## Matrix

### opencode (`opencode` 1.18.1)

| Column | Result | Captured evidence |
|---|---|---|
| `cli_version` | **1.18.1** | `opencode --version` → `1.18.1` (`versions.txt`) |
| `flags_accepted` | Aid passes: `run --format json --thinking` [ `--session` `--continue` `--fork` ] [ `--variant minimal` ] [ `-m` ] [ `--dir` ] [ `-f` ]. All listed in `opencode run --help`. **`--auto` is listed and accepted** but **aid does not pass it**. **`--dangerously-skip-permissions` is NOT in `run --help` but is accepted** (invoke succeeds; unknown `--not-a-real-flag-xyz` dumps help EXIT 1). | `opencode-run-help.txt`, `flag-accept-with-msg.txt` |
| `noninteractive` | `opencode run --format json --thinking [--auto] '<prompt>'` → JSONL (`step_start`, `text`/`reasoning`/`tool_use`, `step_finish`). Aid also sets `needs_pty=true`. | `opencode-default-model.jsonl`, PTY probes |
| `session_resume` | CLI: `-c/--continue`, `-s/--session`, `--fork`. **Aid passes all three when `opts.session_id` is set.** | help; live flag accept with session not fully exercised (fake id) — resume success **UNVERIFIED** |
| `read_only` | No plan/read-only CLI flag. Aid uses **prompt-level** `read_only_prompt` only (warns not enforced). | help has no plan mode; adapter warning path |
| `sandbox` | **No sandbox flag** in `opencode run --help`. | help |
| `model_selection` | **No `-m`:** session export shows default **`opencode-go/glm-5.2`**. `opencode models` lists a large catalog (opencode/*, opencode-go/*, mimo/*, …). Aid passes `-m` only when caller sets model. | `opencode export` → `info.model = {id: glm-5.2, providerID: opencode-go}`; `models-live.txt` |
| `error_envelope` | **Bad model:** `{"type":"error","error":{"name":"UnknownError","data":{"message":"Unexpected server error..."}}}` **Auth:** forced empty HOME hung (EXIT 142 / alarm) — clean auth envelope **UNVERIFIED**. **Quota:** **UNVERIFIED** this session. Nested shape is `error.data.message`, not top-level `message`. | `oc-badmodel.out`, `errors-opencode.txt` |
| `exit_code` | Bad model → **EXIT 1**. Success PONG → **EXIT 0**. | captured runs |
| `attribution` | JSONL `step_finish.part.tokens.{total,input,output,...}` and `part.cost`. Model **not** in stream lines; available via `opencode export` (`info.model`). | default-model jsonl + export |
| `context_injection` | CLI `-f/--file` works **when not eaten by yargs array**. Aid builds `-f <file> <prompt>` → **`Error: File not found: <prompt>`** (prompt consumed as another file). Fixed form `--file=<path> -- '<prompt>'` delivered `SECRET_CONTEXT_TOKEN=AUDIT_OC42` → model answered `AUDIT_OC42`. | `context-flag-order-proof.txt`, `ctx-oc.out` / kilo `AUDIT_OC42` |
| `ratelimit_message` | No live quota text this session. **UNVERIFIED.** Parser looks at top-level `message`/`text` on `type:error` lines — live envelopes nest under `error.data.message`, so detail becomes **`unknown error`** (shape probe on mimo error; same schema on opencode). | `extra-cells.txt` parser probe; no quota capture |

### kilo (`kilo` 7.0.47)

| Column | Result | Captured evidence |
|---|---|---|
| `cli_version` | **7.0.47** | `kilo --version` |
| `flags_accepted` | Aid passes: `run --auto --format json --thinking` (+ session/variant/model/dir/`-f`). `--auto` listed and required for aid path. **`--dangerously-skip-permissions` rejected** (help dump EXIT 1). | `kilo-run-help.txt`, `flag-accept-with-msg.txt` |
| `noninteractive` | `kilo run --auto --format json --thinking '<prompt>'` → JSONL; PONG succeeded. | `kilo-default-model.jsonl` |
| `session_resume` | Same `--continue` / `--session` / `--fork` (+ kilo-only `--cloud-fork` in help). Aid wires the first three. Live resume **UNVERIFIED**. | help |
| `read_only` | No plan flag. Aid prompt-level only for Custom overlay kinds; kilo kind uses same overlay read-only prompt path. | help |
| `sandbox` | **No sandbox flag** in help. | help |
| `model_selection` | **No `-m`:** export shows **`kilo` / `nvidia/nemotron-3-super-120b-a12b:free`**. `kilo models` returned **432** ids including many `:free`. Aid does not force a default model. | export snippet in `exit-codes-and-models.txt`; `extra-cells.txt` |
| `error_envelope` | API/auth/quota dedicated failures **UNVERIFIED** this session (account served free model successfully). Invalid `--dangerously-skip-permissions` → CLI help / EXIT 1 (flag error, not API). | flag probe |
| `exit_code` | Success → **EXIT 0**. Unknown flag → **EXIT 1**. | captured |
| `attribution` | `step_finish` tokens/cost (cost observed `0` on free model). Model via `kilo export`. Reasoning parts may include `metadata.openrouter`. | jsonl + export |
| `context_injection` | Same `-f` array hazard as opencode. With `--file … --`: live reply **`AUDIT_OC42`**. | `ctx-kilo.out` |
| `ratelimit_message` | **UNVERIFIED** (no quota hit). | gap |

### mimocode (`mimo` 0.1.3)

| Column | Result | Captured evidence |
|---|---|---|
| `cli_version` | **0.1.3** (binary name **`mimo`**, path `~/.mimocode/bin/mimo`) | `mimo --version` |
| `flags_accepted` | Aid passes: `run --dangerously-skip-permissions --format json --thinking -m mimo/mimo-auto` (+ optional session/variant/dir/`-f`). Skip-permissions listed in help. **`--auto` rejected** (help dump EXIT 1). | `mimo-run-help.txt`, `flag-accept-with-msg.txt` |
| `noninteractive` | Same `run --format json` JSONL. Aid-like default model call returns error envelope immediately. | `mimo-auto.out` |
| `session_resume` | `--continue` / `--session` / `--fork` present; aid wires them. Live resume **UNVERIFIED**. | help |
| `read_only` | No plan flag; aid prompt-level only. | help |
| `sandbox` | **No sandbox flag**. | help |
| `model_selection` | Aid **always** passes `-m mimo/mimo-auto` when caller omits model. Live: **`APIError` / `Unsupported model mimo-auto` / HTTP 400** even though `mimo models` lists `mimo/mimo-auto` first. Without `-m`, hits configured Xiaomi provider → **401 Invalid API Key**. `mimo/mimo-v2.5` → model-not-found. Catalog lists nvidia/* and `mimo/mimo-auto` only as mimo-prefixed. | `mimo-models-and-tries.txt`, `mimo-auto.out`, `mimo-401.out` |
| `error_envelope` | **API 400:** `{"type":"error","error":{"name":"APIError","data":{"message":"Unsupported model mimo-auto","statusCode":400,...}}}`. **Auth 401:** `Invalid API Key: Please provide valid API Key` (same nested shape). **Quota:** **UNVERIFIED**. | captured outs |
| `exit_code` | API 400 (unsupported model) → process **EXIT 0**. Auth 401 → **EXIT 0**. (Success path not obtained with a working model this session.) | `exit-codes-and-models.txt` |
| `attribution` | Would use same `step_finish` tokens/cost when successful — **UNVERIFIED** (no success turn). | gap |
| `context_injection` | Same family `-f` array issue (not re-proven on mimo after 401). Treat as **same defect class**; live mimo context arrival **UNVERIFIED** due to auth/model failure. | gap + opencode/kilo proof |
| `ratelimit_message` | **UNVERIFIED**. | gap |

---

## Ranked defects

1. **P0 — opencode stalls after `step_start` with no further JSON; idle nudges do not recover or kill**  
   Proven on `t-99cfb89a` (42m) and `t-61567155` (user-killed). Last CLI event is `step_start`; log fills with `Task appears idle…`; no `hung_detected` despite >> default 600s idle. Zero file changes. This is the cascade/hang failure mode called out in the task brief.

2. **P0 — aid opencode omits `--auto` while kilo uses it**  
   Live PTY: without `--auto`, external bash is auto-rejected; with `--auto`, it runs. Opencode help documents `--auto` for autonomous/pipeline use. Aid’s opencode adapter never passes it (only a partial `OPENCODE_CONFIG_CONTENT` allow for `external_directory`).

3. **P0 — context `-f <file> <prompt>` is broken on this CLI family (yargs array eats the prompt)**  
   Live: `opencode run … -f /tmp/oc-ctx-AUDIT.md 'say hi'` → `Error: File not found: say hi`.  
   Live fixed: `--file=/tmp/oc-ctx-AUDIT.md -- '…'` → model returns `AUDIT_OC42`.  
   Aid’s overlay/opencode builders append `-f` then the prompt with no `--` separator — any `--context` dispatch can hard-fail before the agent starts.

4. **P0 — mimocode default model `mimo/mimo-auto` is rejected by the API (400) and the process still exits 0**  
   `mimo models` lists `mimo/mimo-auto`, but run returns `Unsupported model mimo-auto` with **EXIT 0**. Aid forces this model whenever the caller does not override. Same exit-0-with-`type:error` class as the qwen MiniMax finding.

5. **P1 — mimocode auth failure also exits 0**  
   Captured 401 `Invalid API Key` JSONL error with **EXIT 0**. Completion status may still fail via `type:error` detection, but exit_code semantics lie.

6. **P1 — error event detail parser misses nested `error.data.message`**  
   Live envelopes use nested `error.data.message`. Aid’s `parse_json_event` reads top-level `message`/`text` → detail **`unknown error`** (observed on historical opencode failures too). Rate-limit keyword matching on that detail cannot see the real text.

7. **P1 — `--dangerously-skip-permissions` accepted by opencode despite missing from `run --help`**  
   Invoke-with-message succeeded (EXIT 0). Help-only audits would miss it (gemini-family lesson, reverse direction). kilo rejects the same flag; mimo uses it instead of `--auto`.

8. **P2 — permission-flag split across the “family” is easy to get wrong**  
   opencode/kilo: `--auto`. mimo: `--dangerously-skip-permissions`. Sharing an overlay without per-binary flag specs already required custom `extra_args` — still left opencode without an auto-approve flag.

9. **P2 — no sandbox CLI flag on any of the three; read-only is prompt-only**  
   Captured help has neither sandbox nor plan mode. Aid warns for opencode read-only.

10. **P3 — mimocode binary is `mimo`, agent id is `mimocode`**  
    Works when `mimo` is on PATH (`~/.mimocode/bin/mimo` present here). Easy operator confusion; `which mimocode` fails.

---

## Honest gaps

| Gap | Why |
|---|---|
| Live opencode quota / clean auth error text | Account authenticated; empty-HOME auth probe hung rather than returning a clean envelope |
| kilo API/auth/quota envelopes | Free default model served traffic |
| mimocode successful turn / attribution / context arrival | Default model 400; alternate models not found / 401 on Xiaomi provider |
| Session resume round-trip | Flags exist; not proven with a real prior session id |
| Exact kernel wait state of a 50-minute hang (strace/sample) | Historical hangs already finished; live mid-`step_start` hang not left running for 50m in this audit |

---

## Artifacts

Under `/tmp/aid-wg-wg-e3822c9f/opencode-family-audit/`:

- `versions.txt`, `*-help.txt`, `*-run-help.txt`
- `flag-accept-with-msg.txt`, `flag-matrix.txt`
- `pty-bash-noauto.txt`, `pty-bash-auto.txt`, `aidlike-probes.txt`
- `opencode-default-model.jsonl`, `opencode-export.json`, `kilo-default-model.jsonl`
- `mimo-auto.out`, `mimo-401.out`, `mimo-models-and-tries.txt`, `exit-codes-and-models.txt`
- `context-flag-order-proof.txt`, `context-injection2.txt`, `stall-analysis.txt`
- Historical: `~/.aid/logs/t-99cfb89a.jsonl`, `~/.aid/logs/t-61567155.jsonl`
