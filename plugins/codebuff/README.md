# aid-codebuff

Node plugin that exposes the Codebuff SDK through a `aid-codebuff` CLI. It lets `aid` treat Codebuff as an optional streaming agent by translating SDK events into codex-style JSONL.

## Installation
```bash
cd plugins/codebuff
npm install -g .
```

## Prerequisites
- [Get a Codebuff API key](https://www.codebuff.com/profile?tab=api-keys) and populate the `CODEBUFF_API_KEY` environment variable.
- Node.js installed so the CLI can run the bundled SDK.

## Usage
Run the plugin directly while `aid` is configured to point at the wrapper:
```bash
aid-codebuff "Refactor the CLI runner" --cwd . --mode DEFAULT --model "anthropic/claude-opus-4"
```
`aid run` will transparently use this command when `AgentKind::Codebuff` is selected.

## Supported modes
- `DEFAULT` → agent `base2`, cost mode `normal`
- `FREE` → agent `base2-free`, cost mode `free`
- `MAX` → agent `base2-max`, cost mode `max`
- `PLAN` → agent `base2-plan`, cost mode `normal` (for experimentation)

Use `--read-only` to request a read-only run. The plugin is safe to leave installed even if `aid` never routes work through it.
