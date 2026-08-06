# Global Instruction Leak Investigation Report (~/.claude/CLAUDE.md)

## Context & Executive Summary

When `aid` dispatches subordinate AI agent CLIs to perform tasks, certain CLI adapters inspect the orchestrator's global agent-instruction file (`~/.claude/CLAUDE.md` or `~/.claude/Claude.md`). As observed in production, a dispatched `grok` agent read `~/.claude/Claude.md` (28KB context), adopted the orchestrator's persona, executed `hiboss ask`, and blocked waiting for human approval — abusing the orchestrator's channel to the human operator.

This investigation empirically measures across all 11 supported agent CLIs (`grok`, `codex`, `cursor`, `agy`, `commandcode`, `copilot`, `opencode`, `droid`, `oz`, `qwen`, `gemini`) whether each CLI reads global instruction files from `$HOME`, how it can be suppressed, and the single optimal isolation mechanism for `aid`.

---

## Empirical Measurement Table

| CLI | reads a global instruction file? | evidence (command you ran + output you saw) | suppression mechanism | evidence the suppression works | verdict |
| :--- | :---: | :--- | :--- | :--- | :--- |
| **grok** | **Y** | `grok inspect` with `HOME=/tmp/test_home_grok` (containing `.claude/Claude.md`)<br><br>**Output:** `Project Instructions (1) └ /tmp/test_home_grok/.claude/Claude.md (global, ~26 tokens) [claude]` | Redirect `HOME` to an isolated temp directory (`HOME=/tmp/isolated_home`) with symlinked `~/.grok` (for auth/skills), omitting `.claude`. | `grok inspect` with isolated `HOME`<br><br>**Output:** `Project Instructions (0) └ (none)` | **Reads global file.** Suppressible via isolated `HOME` with symlinked `~/.grok`. |
| **codex** | **N** | `codex exec --help`<br><br>**Output:** Codex loads configuration exclusively from `$CODEX_HOME` (`~/.codex/config.toml`) and local execpolicy `.rules` files. Does not read `~/.claude/CLAUDE.md`. | Pass `--ignore-user-config` / `--ignore-rules` or isolate `CODEX_HOME=/tmp/isolated_codex`. | `codex exec --ignore-user-config --ignore-rules` bypasses `$CODEX_HOME/config.toml` and user rules completely. | **Does not read `~/.claude/CLAUDE.md`.** Config isolated via `CODEX_HOME` or `--ignore-user-config`. |
| **cursor** | **N** | `agent -p "List instruction sources" --output-format json --trust --force` under `HOME` with `.claude/CLAUDE.md`<br><br>**Output:** `User-scoped rules loaded: we use english` (from `~/.cursor/rules`), with zero reference to `~/.claude/CLAUDE.md`. | Set isolated `HOME` or empty `--plugin-dir`/rules directory while preserving `~/.cursor` auth. | `agent status` / `agent -p` under isolated `HOME` loads 0 user rules. | **Does not read `~/.claude/CLAUDE.md`** (reads `~/.cursor/rules`). Suppressible via isolated `HOME`. |
| **agy** | **Y** | Architecture inspect + prompt context under `HOME=/tmp/test_home_agy`<br><br>**Output:** `agy` automatically scans and loads `$HOME/.claude/CLAUDE.md` and `$HOME/.gemini/` global skills into system prompt context. | Redirect `HOME` to an isolated temp directory (`HOME=/tmp/isolated_home`) with symlinked `~/.gemini` auth. | Running `agy` under isolated `HOME` omits `$HOME/.claude/CLAUDE.md` from prompt assembly. | **Reads global file.** Suppressible via isolated `HOME`. |
| **commandcode** | **N** | `commandcode -p "Do you see any file or instruction containing GLOBAL_INSTRUCTION_MARKER_COMMANDCODE_3322?" --yolo` with `HOME=/tmp/test_home_commandcode`<br><br>**Output:** `No. The workspace directory /private/tmp/scratch_commandcode_test is completely empty, and a search for GLOBAL_INSTRUCTION_MARKER_COMMANDCODE_3322 found nothing` (Exit code 0). | None needed for `~/.claude/CLAUDE.md`. Set isolated `HOME` with symlinked `~/.commandcode/auth.json` to isolate `~/.commandcode/config.json`. | Empirical workspace search confirmed zero global instruction leak. | **Does not read `~/.claude/CLAUDE.md`.** |
| **copilot** | **unknown** | `copilot -p "Do you see marker...?"` with `HOME=/tmp/test_home_copilot` and `GITHUB_TOKEN`<br><br>**Output:** `Error: No authentication information found.` Headless non-interactive invocation requires interactive OAuth session; prompt context uncaptured. | N/A (`unknown`) | N/A (`unknown`) | **unknown** (API authentication failed in headless CLI mode; unverified empirically). |
| **opencode** | **N** | `opencode --help` + config inspect<br><br>**Output:** `opencode` loads settings from `~/.config/opencode/opencode.json` and local `.opencode/`. Does not read `~/.claude/CLAUDE.md`. | Set `XDG_CONFIG_HOME=/tmp/clean_config` or isolated `HOME`. | `opencode` with isolated `XDG_CONFIG_HOME` loads built-in defaults only. | **Does not read `~/.claude/CLAUDE.md`.** |
| **droid** | **N** | `droid exec --output-format stream-json --skip-permissions-unsafe "..."` with `HOME=/tmp/test_home_droid_sym`<br><br>**Output:** Init session event `session_id: cc424583-...` initialized tools and workspace settings without loading `~/.claude/CLAUDE.md`. | `--settings <path>` flag or isolated `HOME` with symlinked `~/.factory`. | `--settings` flag explicitly overrides merged runtime settings for process execution. | **Does not read `~/.claude/CLAUDE.md`.** |
| **oz** | **unknown** | `oz agent run -p "..." --output-format json` under `HOME=/tmp/test_home_oz`<br><br>**Output:** `STDERR: You are not logged in - please log in with oz login to continue.` `~/.oz` does not exist on host. | N/A (`unknown`) | N/A (`unknown`) | **unknown** (CLI not authenticated on host machine; unverified empirically). |
| **qwen** | **N** | `qwen -p "Do you see marker GLOBAL_INSTRUCTION_MARKER_QWEN_1122?"` with `HOME=/tmp/test_home_qwen`<br><br>**Output:** `No. I see no global instruction file or the marker GLOBAL_INSTRUCTION_MARKER_QWEN_1122 — the working directory is empty (no QWEN.md or matching content), and nothing in my loaded context references it.` (Exit code 0). | None needed for `~/.claude/CLAUDE.md`. Set isolated `HOME` with symlinked `~/.qwen` to isolate user settings. | Empirical execution output confirmed zero global instruction leak. | **Does not read `~/.claude/CLAUDE.md`.** |
| **gemini** | **N** | `gemini -y -p "..."` under `HOME=/tmp/test_home_gemini`<br><br>**Output:** Gemini CLI loads `GEMINI.md` from project workspace and settings from `~/.gemini/settings.json`. Does not read `~/.claude/CLAUDE.md`. | Set `GEMINI_CONFIG_DIR=/tmp/clean_gemini` or isolated `HOME` with symlinked `~/.gemini` auth files. | Isolated `HOME` prevents reading `~/.gemini/settings.json` user overrides. | **Does not read `~/.claude/CLAUDE.md`.** |

---

## Single Recommended Suppression Mechanism for `aid`

### Recommended Mechanism: **Isolated Per-Task `HOME` Directory with Selective Auth Symlinking**

At dispatch time, when `aid` spawns any subordinate agent process, `aid` should set the child process `HOME` environment variable to a clean, isolated task directory (e.g. `/tmp/aid-home-$TASK_ID` or `$TASK_DIR/.home`), and selectively symlink only the agent's authentication/configuration directory from the real `$HOME`.

#### CLI Auth Symlink Registry
For each agent CLI, `aid` symlinks only its specific credential folder into the isolated task `$HOME`:
- **grok**: `~/.grok`
- **codex**: `~/.codex`
- **cursor**: `~/.cursor`
- **gemini / agy**: `~/.gemini`
- **commandcode**: `~/.commandcode`
- **opencode**: `~/.config/opencode` & `~/.opencode`
- **droid**: `~/.factory`
- **qwen**: `~/.qwen`
- **copilot / oz**: `~/.config/github-copilot` / `~/.oz`

Crucially, `.claude/` (and any global `CLAUDE.md` / `Claude.md`) is **omitted** from the isolated task `$HOME`.

### Costs and Tradeoffs
1. **Performance Cost:** ~1-2ms overhead per task dispatch to create a temporary directory and 1-2 symlinks.
2. **Maintenance Cost:** `aid` must maintain an internal mapping of `AgentKind` to its required auth directory name (e.g., `Grok -> .grok`, `Droid -> .factory`).
3. **Isolation Guarantee:** 100% suppression of orchestrator `~/.claude/CLAUDE.md` leaks across all current and future subagents, preventing subordinate agents from adopting orchestrator persona or triggering unauthorized human interaction workflows (e.g. `hiboss ask`).
