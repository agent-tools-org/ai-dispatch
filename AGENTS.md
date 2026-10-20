# Agent commands

Built-in run agents: `agy`, `qwen`, `codex`, `copilot`, `opencode`, `commandcode`, `cursor`,
`kilo`, `mimocode`, `droid`, `oz`, `claude`, and `grok`.

Use `aid run agy "Research the module" -o notes.md` for a research task.
`aid ask`, `aid show --explain`, and bare `--judge` default to `agy`.

The `gemini` agent was removed. Use `agy`, a Google provider through
`opencode`/`kilo`, or a custom agent. Historical tasks retain their original names.
