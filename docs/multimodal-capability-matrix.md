# Multimodal capability matrix

Measured 2026-08-13 against `aid 10.29.0 (b8d8b66d)`. Scope is every entry returned by `aid agent list`, including entries currently disabled or unavailable. No generation task, installation, or paid API call was used.

## Reading the matrix

`Y` means the dispatched route exposes a measured path. `N` means the aid wrapper or script demonstrably does not expose that path. `U` means UNKNOWN: the cheap probe did not settle it. UNKNOWN is intentional and must not be treated as a negative capability. `—` means the route is not currently dispatchable. A suffix such as `Y†` is explained below the table.

Evidence keys are how each cell was established:

- **P** — Read-only `--version`, `--help`, and relevant subcommand help on the installed binary. The aid adapter is linked in the route column. Help absence produces `U`, never `N`.
- **G** — Gemini CLI documentation says direct `@` file access supports images, audio, and PDF, while custom-command `@{path}` injection also documents video: [tools reference](https://geminicli.com/docs/reference/tools/), [custom commands](https://geminicli.com/docs/cli/custom-commands/).
- **Q** — Qwen Code documents `read_file`/multi-file handling for image, PDF, audio, and video files, conditional on the current model supporting the modality: [file-system tools](https://qwenlm.github.io/qwen-code-docs/en/developers/tools/file-system/), [multi-file read](https://qwenlm.github.io/qwen-code-docs/en/developers/tools/multi-file/). The configured model’s modality support was not probed.
- **O** — OpenCode’s `run --help` and [CLI reference](https://opencode.ai/docs/cli/) expose `--file/-f` as “file(s) to attach to message”; no accepted media-type list was exposed.
- **F** — Codex `exec --help` exposes `-i/--image <FILE>...`.
- **X** — `codex mcp list` found `computer-use` disabled/Unsupported and `node_repl` enabled/Unsupported; this is not evidence of working desktop or browser control.
- **N** — [nanobanana.toml](/Users/mingsun/.aid/agents/nanobanana.toml) invokes [nanobanana-gen.sh](/Users/mingsun/.aid/scripts/nanobanana-gen.sh), whose only MCP call is `generate_image` and whose arguments are prompt plus output directory. The installed extension does contain edit tools, but this aid route does not call them: [extension manifest](/Users/mingsun/.gemini/extensions/nanobanana/gemini-extension.json), [edit command](/Users/mingsun/.gemini/extensions/nanobanana/commands/edit.toml).
- **V** — [videogen.toml](/Users/mingsun/.aid/agents/videogen.toml) invokes [videogen.sh](/Users/mingsun/.aid/scripts/videogen.sh), which asks Gemini for a VHS tape and invokes `vhs` to render an MP4; it accepts only a prompt.

## Measured matrix

| Agent (aid state) | Dispatched route / probe source | Image input | Image generation | Video generation | Audio input | Computer use (OS/desktop) | Browser control | File / PDF input |
|---|---|---:|---:|---:|---:|---:|---:|---:|
| gemini (installed, disabled) | `gemini`; [gemini.rs](../src/agent/gemini.rs), [gemini_support.rs](../src/agent/gemini_support.rs); P/G/N | Y [G] | Y† [N] | U [P] | Y [G] | U [P] | U [P] | Y [G] |
| qwen (installed) | `qwen`; [qwen.rs](../src/agent/qwen.rs); P/Q | U* [Q] | U [P] | U* [Q] | U* [Q] | U [P] | U [P] | U* [Q] |
| codex (installed) | `codex exec`; [codex.rs](../src/agent/codex.rs); P/F/X | Y [F] | U [P] | U [P] | U [P] | U [X] | U [P] | U [P] |
| copilot (installed, limited) | `copilot`; [copilot.rs](../src/agent/copilot.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| opencode (installed) | `opencode run`; [opencode.rs](../src/agent/opencode.rs); P/O | U [O] | U [P] | U [P] | U [P] | U [P] | U [P] | U [O] |
| commandcode (installed) | `commandcode`; [commandcode.rs](../src/agent/commandcode.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| cursor (not installed) | `cursor-agent` missing; [cursor.rs](../src/agent/cursor.rs) | — | — | — | — | — | — | — |
| kilo (installed) | `kilo run` via OpenCode overlay; [kilo.rs](../src/agent/kilo.rs), [opencode_overlay.rs](../src/agent/opencode_overlay.rs); P/O | U [O] | U [P] | U [P] | U [P] | U [P] | U [P] | U [O] |
| mimocode (installed, disabled) | `mimo` overlay; [mimocode.rs](../src/agent/mimocode.rs), [opencode_overlay.rs](../src/agent/opencode_overlay.rs); P/O | U [O] | U [P] | U [P] | U [P] | U [P] | U [P] | U [O] |
| droid (installed, limited) | `droid exec`; [droid.rs](../src/agent/droid.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| oz (installed) | `oz agent run`; [oz.rs](../src/agent/oz.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| agy (installed) | `agy`; [antigravity.rs](../src/agent/antigravity.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| grok (installed) | `grok`; [grok.rs](../src/agent/grok.rs); P | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] | U [P] |
| goose (not installed) | `goose` missing; [goose.toml](/Users/mingsun/.aid/agents/goose.toml) | — | — | — | — | — | — | — |
| mimo (installed, disabled) | Custom OpenCode overlay forced to `mimo/mimo-v2.5-pro`; [mimo.toml](/Users/mingsun/.aid/agents/mimo.toml), [opencode_overlay.rs](../src/agent/opencode_overlay.rs); P/O | U [O] | U [P] | U [P] | U [P] | U [P] | U [P] | U [O] |
| nanobanana (installed) | Bash → direct Nano Banana MCP; [nanobanana.toml](/Users/mingsun/.aid/agents/nanobanana.toml), [nanobanana-gen.sh](/Users/mingsun/.aid/scripts/nanobanana-gen.sh); N | N [N] | Y [N] | N [N] | N [N] | N [N] | N [N] | N [N] |
| ollama (installed) | Custom OpenCode overlay forced to `ollama/qwen3:4b`; [ollama.toml](/Users/mingsun/.aid/agents/ollama.toml), [opencode_overlay.rs](../src/agent/opencode_overlay.rs); P/O | U [O] | U [P] | U [P] | U [P] | U [P] | U [P] | U [O] |
| videogen (installed) | Bash → Gemini → VHS; [videogen.toml](/Users/mingsun/.aid/agents/videogen.toml), [videogen.sh](/Users/mingsun/.aid/scripts/videogen.sh); V | N [V] | N [V] | Y [V] | N [V] | N [V] | N [V] | N [V] |

† Gemini’s Nano Banana extension is installed and enabled for the user/workspace (`gemini extensions list`), but the probe folder was untrusted and Gemini reported its MCP server disabled there. `aid` sets `GEMINI_CLI_TRUST_WORKSPACE=true` in the Gemini adapter and its isolated HOME preserves `.gemini`, so image generation is a conditional measured route, not a claim about every Gemini installation: [gemini.rs](../src/agent/gemini.rs), [home_isolation.rs](../src/agent/home_isolation.rs), [gemini-extension.json](/Users/mingsun/.gemini/extensions/nanobanana/gemini-extension.json).

\* Qwen’s documented media-reading tool is conditional on model support; the current `coder-model`/provider was not queried. This is UNKNOWN, not a capability claim about Qwen3-Coder.

### MCP and extension observations

The only installed extension found by `HOME=/Users/mingsun gemini extensions list` was `nanobanana` 1.0.12, with one configured MCP server; the server was disabled in the untrusted probe folder. `codex mcp list` showed the computer-use server disabled/Unsupported and node-repl enabled/Unsupported. Grok and Command Code reported no configured MCP servers. OpenCode’s MCP listing failed on its local log path, so its configured-server state remains UNKNOWN. These observations are from read-only list commands; no server was started.

The current routing gap is also observable: both `aid advise --difficulty simple --budget free --urgency normal --rigor standard --json "generate an image ..."` and the equivalent video prompt inferred `research`, recommended `grok`, and gave `nanobanana` and `videogen` category capability `0` with `base 0 < floor 4`. This is a routing observation, not evidence that Grok generates media.

## Proposal: carry modality as measured route metadata

Keep the existing task category for text/code fit. It currently has exactly eight values—Research, SimpleEdit, ComplexImpl, Frontend, Debugging, Testing, Refactoring, and Documentation—in [classifier.rs](../src/agent/classifier.rs#L8-L19), and the built-in score table is category-only in [selection_capabilities.rs](../src/agent/selection_capabilities.rs#L12-L97). Do not turn image or video into another 0–10 category: modality is a hard route requirement, not a style of coding work.

Add a separate, route-scoped metadata record with two orthogonal sets:

1. `input_modalities`: text, image, audio, video, file, PDF.
2. `actions`: image_generation, video_generation, audio_generation, browser_control, computer_use.

Each entry should be `supported`, `unsupported`, or `unknown`, with evidence text, probe command, CLI version, and conditions such as “extension enabled” or “model-dependent.” Keep `browser_control` separate from web search/fetch, and keep `computer_use` separate from shell execution. Put this beside the agent adapter/capability map, not in the model name or provider metadata. Custom agents need the same schema in their registry metadata; the current `CapabilityScores` has no modality fields in [custom.rs](../src/agent/custom.rs#L83-L101), which is why `design = 10` in the Nano Banana TOML is not a usable dimension.

`aid advise` should extract requirements from explicit attachments, file extensions, flags, and clear verbs (“generate an image”, “control the browser”), then apply modality as a hard filter before category scoring. A supported route may be recommended; an unsupported route is excluded; an unknown route is shown as “unverified” and never silently treated as zero or yes. If no supported route remains, advise should say so and name the missing probe/evidence. The existing category ranking and history can break ties after this filter; [selection_advice.rs](../src/agent/selection_advice.rs#L82-L121) currently has no such stage.

This preserves evidence-first routing: a stale or conditional declaration can be re-probed per CLI version, and `aid advise` can explain both the recommendation and the exact reason a media route was excluded.

## Sources and reproducibility

- `aid agent list --json`, `aid agent show <name>`, `aid --version`; read-only local probes run 2026-08-13.
- `aid advise --difficulty simple --budget free --urgency normal --rigor standard --json "generate an image ..."` and the analogous video prompt; both were read-only probes on 2026-08-13.
- Native route construction: [src/agent](../src/agent/), especially the adapter links in the table.
- Custom route definitions and wrappers: `/Users/mingsun/.aid/agents/*.toml` and `/Users/mingsun/.aid/scripts/*.sh` linked above.
- [OpenAI Codex model modality reference](https://developers.openai.com/api/docs/models/gpt-5.2-codex) was used only as context; it was not used to infer the current Codex CLI route’s capabilities.
