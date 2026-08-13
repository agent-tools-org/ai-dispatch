# Investigation: Qwen Retry Session Failure

## Problem
An `aid retry` of a `qwen` task (t-b457d028, retrying t-9ecc0a51) exits in ~2s with code 1 and writes no output to the log. The task's `dispatch_args` show `"dir": null`.

## Evidence
We tested the two competing hypotheses with direct `qwen` probe commands (`qwen -r ec9e3217-6444-424c-b21a-1d026d22928d -p 'reply ok'`) to observe its behavior.

1. **Testing H1 (Different CWD)**
   Command:
   ```bash
   cd /Users/mingsun/Develop/ai/ai-dispatch
   qwen -r ec9e3217-6444-424c-b21a-1d026d22928d -p 'reply ok' > out 2> err
   ```
   Output: `exit_code=1`
   Stderr: `No saved session found with ID ec9e3217-6444-424c-b21a-1d026d22928d. Run qwen --resume without an ID to choose from existing sessions.`
   *Implication*: `qwen` relies strictly on the current working directory to hash the project path and locate the session. If the cwd changes, it cannot find the session.

2. **Testing H2 (Isolated HOME)**
   Command:
   ```bash
   mkdir -p /tmp/fake_home
   cd /Users/mingsun/Develop/web3/swap-solver/uniswapx-filler/uniswapx-filler-rs
   HOME=/tmp/fake_home qwen -r ec9e3217-6444-424c-b21a-1d026d22928d -p 'reply ok' > out 2> err
   ```
   Output: `exit_code=1` (fails to find session because the new home is empty).
   *Code Evidence*: We inspected `aid`'s home isolation logic in `src/agent/home_isolation.rs:142-168`. `IsolatedHomeGuard` explicitly symlinks entries from the operator's real `$HOME` into the isolated task home, skipping only those in `DEFAULT_DENYLIST` (`[".anthropic", ".agents", ".agent"]`).
   *Implication*: Since `.qwen` is NOT in the denylist, the isolated HOME environment successfully symlinks `~/.qwen`. The agent has full access to the operator's Qwen sessions. H2 is dead.

## Root Cause
The evidence kills **H2** (isolated HOME) and confirms **H1** (cwd).

When the original task ran, `dir` was `null` (not explicitly provided). `qwen` inherited the shell's cwd (`uniswapx-filler-rs`), hashing it to store the session at `~/.qwen/projects/-Users-mingsun-Develop-web3-swap-solver-uniswapx-filler-uniswapx-filler-rs/chats/ec9e3217...`.

When `aid retry` was invoked from a different directory (e.g., `ai-dispatch`), it loaded the `null` dir from the database and spawned `qwen` in the *new* inherited cwd (`ai-dispatch`). `qwen` hashed the new cwd, looked for the session in `~/.qwen/projects/-Users-mingsun-Develop-ai-ai-dispatch/`, failed to find it, and gracefully exited 1 with a stderr message (which `aid`'s stream-json parser ignored, resulting in a 0-byte log).

## Fix Options

1. **Explicit CWD Capture (Recommended)**
   If an operator omits `--dir`, `aid` should capture the absolute `current_dir` during dispatch and save it to `dispatch_args.dir`. This guarantees retries run in the exact same directory regardless of where `aid retry` is called from.
   *Location*: `src/cmd/run.rs` (around the instantiation of `RunArgs`).

2. **Fallback to task.repo_path in retry**
   In the retry handler, if `run_args.dir` is `None`, fall back to the task's recorded repository path. This is a simpler fix but loses subdirectory precision if the original command was run deep inside a workspace.
   *Location*: `src/cmd/retry.rs:111`
   *Change*: Add `run_args.dir = run_args.dir.or_else(|| task.repo_path.clone());`
