# Agent commands

Built-in run agents: `agy`, `codex`, `copilot`, `opencode`, `commandcode`, `cursor`,
`kilo`, `mimocode`, `droid`, `oz`, `claude`, and `grok`.

Use `aid run agy "Research the module" -o notes.md` for a research task.
`aid ask`, `aid show --explain`, and bare `--judge` default to `agy`.

The `gemini` and `qwen` agents were removed. Use `agy`, a Google provider through
`opencode`/`kilo`, or a custom agent. Historical tasks retain their original names.
