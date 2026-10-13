# Website and README refresh — stale facts and proposed story

Checked 2026-10-13 against v10.52.1 (`9062bd08`): the live page and `/llms.txt` at
`aid.agent-tools.org`, and `README.md`. The site's source is not in this repository.

## Stale facts

| Where | Says | Current |
|-------|------|---------|
| Site hero, `/llms.txt` | `v8.78.0` | 10.52.1 |
| Site hero | "5 built-in agents" | 14: gemini, agy, qwen, codex, copilot, opencode, commandcode, cursor, kilo, mimocode, droid, oz, claude, grok |
| Site, `/llms.txt` description | "gemini, codex, opencode, cursor, kilo, and custom agents" | as above; `agy` replaces gemini for Google One / Code Assist individual tiers |
| Site "Auto Selection" | picks the agent "from capability scores and history" | no `auto` agent; `aid advise` ranks routes from third-party leaderboards (Terminal-Bench, Epoch ECI, SWE-bench Verified, LMArena) and live quota; aid assigns no scores of its own |
| Site commands table | `output`, `completions` | removed in v10.50.0; use `aid show --output`, `aid notifications` |
| Site commands table | missing | `advise`, `accept`, `reject`, `gc`, `unstick`, `doctor`, `byok`, `credential`, `kg`, `cost`, `errors` |
| Site `run` row | `--timeout` without a unit | seconds (`--timeout 5400` for 90 minutes) |
| Site feature ages | "NEW" on experiment/judge/cascade/best-of; per-feature `v8.x` tags | v8 features are baseline; tag current ones instead |
| `README.md` badge and "Project status" | 10.47.0 | 10.52.1; `scripts/release.sh` does not update either, so both drift every release |

## What has landed since the site was written

Group the page around these rather than the v8 feature list:

- **Routing from evidence**: `aid advise` with declared difficulty/budget/urgency/rigor,
  third-party capability feeds, per-model quota windows, cascade to the next advised
  route.
- **Quota awareness**: per-agent and per-model-group holds parsed from provider
  refusals, with reset times; held routes are skipped at dispatch.
- **Custody**: every task's work is preserved — worktree branches, salvage commits on
  failure, recovery refs for shared checkouts; `aid accept` then `aid gc` releases it.
- **Remote builds**: `--remote-build` runs Cargo on an rbox build box.
- **Read-only audits**: `--read-only` with a saved Markdown report and verdict.
- **Clients**: optional authenticated HTTP API/SSE server (`web` feature) and the
  macOS/iPadOS AID Command app.
- **Operating guide**: `aid-guide` skill installed by `aid init`, generated from code.

## Rules for the copy

- Technical facts only; no internal process, usage statistics or plans.
- Generate version, agent list and command table from the binary (`aid --version`,
  `aid config agents`, `aid --help`) at release time instead of hand-editing.
- Keep the security notice; add that `--read-only` is enforced by snapshot comparison
  for adapters without a native read-only mode.
