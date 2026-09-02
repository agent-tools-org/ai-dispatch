# qwen delivers nothing: aid never passes an approval-mode flag

**Status:** root cause confirmed by controlled A/B against the real CLI.
**Reported symptom:** "qwen tasks always fail to deliver — is it a CLI compatibility problem?"
**Answer:** Yes. It is a CLI contract change plus an aid adapter that never carried the flag.

`KB consulted:` `kb qwen cli agent delivery failure` — **no match** for qwen/approval-mode/tool-gating.
Nearest hits were generic (`delivery-status-masks-provider-failure.md`,
`agent-dispatch-hygiene.md`); neither describes this failure. Memory
`project_agent_failures_2026_08_06` item 2 ("qwen produced no output for 180s after emitting a
nested sub-agent call") is the same bug seen from the outside — see *Closure* below.

## Evidence

### 1. Every qwen delivery is empty, and has been since 2026-08-06

`~/.aid/aid.db`, `tasks` where `agent='qwen'` (74 rows, 2026-05-02 → 2026-09-02):

| files_changed | rows |
|---|---|
| `[]` (empty) | 63 |
| non-empty | 5 |
| no summary | 6 |

The last row with a non-empty `files_changed` is `t-a52ddc5c` (2026-08-06). **Every one of the 27
qwen tasks dispatched since 2026-08-07 changed zero files.**

The five non-empty rows are not counter-evidence. `t-f263da10` (2026-08-05) changed one file and
concluded verbatim: *"this task needs to be run in an environment that provides `write_file` or
shell access."* `t-de472803` (2026-08-06) concluded *"Let me launch a comprehensive agent that
makes all remaining file edits at once."* qwen had no write tool then either — it was delegating
writes to nested `agent` sub-agents. Those task logs have since been GC'd, so the exact upstream
version at which the toolset changed is **not established**.

### 2. The dispatched agent has no mutating tools

`~/.aid/logs/t-72472cd9.jsonl` (2026-09-02, most recent qwen task), first line:

```
qwen_code_version : 0.22.3
permission_mode   : auto
tools             : tool_search, task_stop, get_goal, update_goal, list_agents,
                    read_mcp_resource, skill, read_file, zoom_image, grep_search,
                    report_findings, glob, agent, enter_worktree, todo_write,
                    exit_worktree, send_message, web_fetch, record_artifact,
                    cron_create, loop_wakeup, cron_list, cron_delete
```

No `write_file`, no `edit`, no `run_shell_command`. In `t-0e58d615` the agent searched for them
itself — `tool_search{"query":"shell command execute bash"}`,
`tool_search{"query":"write file edit create"}` — found none, and its `agent` calls came back
*"permission was declined (non-interactive mode cannot prompt for confirmation)"*.

### 3. A/B control against the real CLI — the decisive test

Same prompt, same model, same empty directory. Only the flag differs.

| | flags | `permission_mode` | mutating tools | `probe.txt` | exit |
|---|---|---|---|---|---|
| **A** (what aid sends) | `-o stream-json -m qwen3.8-max -p …` | `auto` | none | **not created** | **0** |
| **B** | `--approval-mode yolo` + same | `yolo` | `edit`, `write_file`, `run_shell_command`, `notebook_edit`, `monitor` | **`OK`** | 0 |

Probe dirs: `…/scratchpad/qwenprobe-A`, `…/scratchpad/qwenprobe-B`.

Run A's own last event: *"To unblock: Re-run this in a session/profile with a write-capable tool
enabled … I'm reporting this honestly rather than claiming success."* It **exits 0**, which is why
aid records the task `done`.

Run A reproduces the failure with aid entirely out of the picture — so the isolated `$HOME`, the
worktree, and aid's watcher are all excluded as causes.

## Root cause

`src/agent/qwen.rs:38-52` builds the `qwen` command by hand and passes **no approval-mode flag**:

```rust
let mut cmd = Command::new("qwen");
cmd.args(["-o", "stream-json"]);
cmd.args(["-m", &model]);
if opts.sandbox { cmd.arg("--sandbox"); }
```

Its own file header claims it "reuses Gemini support helpers for CLI flags" — it does not. It
reuses only the *parsing* helpers. The command builder that does handle this,
`gemini_support::build_gemini_command` (`src/agent/gemini_support.rs:29-38`), is used by gemini and
agy and gets it right:

```rust
if opts.read_only && !allow_result_file_write(opts) {
    cmd.args(["--approval-mode", "plan"]);
} else {
    cmd.arg("-y");
}
```

qwen 0.22.3 defaults non-interactive `-p` runs to `permission_mode: auto`, which ships a read-only
toolset. `--approval-mode yolo` (still supported; merely absent from `qwen --help`) restores
`write_file` / `edit` / `run_shell_command`. aid has never sent it, so every qwen dispatch has been
silently read-only.

This is the `one-rule-many-implementations` shape: the rule was implemented once for the gemini
family and the qwen adapter was written around it rather than through it.

## Closure of an older finding

`project_agent_failures_2026_08_06` item 2 recorded qwen hanging 180s after emitting a nested
`agent` sub-agent call, root cause *"not established — the log ends at the call."* It is
established now: qwen could not write files itself, so it delegated the writes to a sub-agent, and
in non-interactive mode that sub-agent call is auto-declined (*"permission was declined
(non-interactive mode cannot prompt for confirmation)"*) or stalls. The hang was a symptom of the
missing approval flag.

## Fix

Route qwen through `gemini_support::build_gemini_command`, or mirror its approval-mode branch.
Because qwen now has a genuine read-only mode, the `bail!` at `src/agent/qwen.rs:35` ("qwen agent
does not support read-only mode") should be replaced by `--approval-mode plan` — that is a public
flag behaviour change and needs the aid-guide updated in the same commit.

**Acceptance gate is not a unit test.** The assertion that `--approval-mode yolo` appears in the
args is table stakes; the real gate is one live dispatch under the isolated `$HOME`:
`aid run qwen "<create a file>" -w test/qwen-approval` → `completion_summary.files_changed`
non-empty. That is the check that has been failing silently for four weeks.

Also check whether aid's stderr capture treats qwen's yolo banner as an error event
(`Warning: running headless with --yolo / approval-mode=yolo and no sandbox…`); if it does, set
`QWEN_CODE_SUPPRESS_YOLO_WARNING=1` in the child env.

## Separate defects found alongside — NOT part of this fix

1. **Quota exhaustion recorded as `done`, exit 0.** `t-c25a39df`, `t-ad1e3b0f`, `t-9a8caccd`,
   `t-ad79ccb3`, `t-78f1e496`, `t-b4f9926c`, `t-b81a6646`, `t-d3c61216`, `t-f59497ca`,
   `t-803822ec` — all 4-15s runs whose conclusion is
   `insufficient_quota: 429 Your token-plan 1-week quota has been exhausted`, all stored `done`.
   qwen's `streaming()` is `true`, so `parse_completion` is dead code for it
   (`project_streaming_ignores_parse_completion`) — editing it will compile, test green, and change
   nothing. Needs the streaming path.
2. **`[Unrecognized JSON log format from unknown agent]`** on `t-d536a254` and `t-026de1ef`, whose
   sample line is qwen's own `{"type":"system","subtype":"init",…}`. Classifier gap, cosmetic.
