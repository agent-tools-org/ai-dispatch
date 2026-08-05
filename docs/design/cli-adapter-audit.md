# CLI adapter audit — verification matrix

Status: active (2026-08-05)

## Why

qwen was 100% broken for months: `src/agent/qwen.rs` reused the gemini-family command builder,
which passes `-y`, `--approval-mode`, and `--include-directories` — none of which the qwen CLI
accepts. Every dispatch died on an invalid flag. Our test suite stayed green throughout, because
the tests exercise our own fixtures rather than the installed CLIs.

That is the shape of the risk: **we have never verified our adapters against the CLIs they drive.**
agy shares the same builder and happens to work; nobody has checked whether that is compatibility
or coincidence. The model catalog carries one stale free-tier entry for a 17-model plan. Capability
scores are hand-authored integers that gate eligibility. Rate-limit recovery times are misparsed
("resets in 1 day" displayed as "~1h").

## Rule

Every cell in this matrix must be filled from **real captured output of the installed CLI**.
A conclusion drawn from reading aid's source, or from a unit test, does not count and must be
marked UNVERIFIED instead. Where a CLI cannot be exercised (no credentials, exhausted quota), say
so explicitly — an honest gap is worth more than an assumed pass.

## Matrix

One row per agent. Columns:

| Column | What to capture |
|---|---|
| `cli_version` | The installed version, from the binary |
| `flags_accepted` | Which flags aid passes, and whether `--help` lists each one |
| `noninteractive` | The exact invocation for a one-shot run, and its output format |
| `session_resume` | Whether resume exists, the flag, and whether aid uses it |
| `read_only` | Whether a plan/approval/read-only mode exists, and its flag |
| `sandbox` | Whether a sandbox flag exists, and what it restricts |
| `model_selection` | How the model is chosen when the caller names none; what the CLI defaults to; which models the account/plan actually serves |
| `error_envelope` | The literal shape of an API error, an auth error, and a quota error |
| `exit_code` | The process exit code for each of those three failures |
| `attribution` | Which fields carry model, tokens, and cost in the output |
| `context_injection` | How context files reach the agent, and whether they arrive |
| `ratelimit_message` | The literal quota message, and whether aid parses its reset time correctly |

## Families

Adapters that share a builder must be audited together — that is where the qwen bug lived.

- gemini family: `gemini`, `agy`, `qwen` (`build_gemini_family_command`)
- opencode family: `opencode`, `kilo`, `mimocode` (`opencode_overlay`)
- codex family: `codex`, `codebuff`
- standalone: `cursor`, `copilot`, `droid`, `oz`, `claude`

## Deliverable

A filled matrix per family, with the captured output that justifies each cell, plus a ranked list
of defects found. Defects are not fixed in the audit — they are reported, so the fix can be
scoped and verified separately.
