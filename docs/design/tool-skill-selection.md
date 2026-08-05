[MILESTONE] Codebase investigation complete — mapped tool/skill injection mechanisms and adapter capabilities.

[MILESTONE] Analyzed caller surface requirements and honest defaults.

[MILESTONE] Verified prompt text injection mechanism against CLI adapter capabilities.

# Design Proposal: Caller-Declared Tool and Skill Selection

**Status**: Proposal  
**Author**: Antigravity  
**Target System**: `aid` (`ai-dispatch`)  

---

## 1. Executive Summary & Problem Statement

Currently, `aid` uses keyword- and prompt-length heuristics to infer task categories via `agent::classifier::classify` ([src/agent/classifier.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/agent/classifier.rs#L160)), then filters custom toolbox tools using `toolbox::filter_by_task_category` ([src/cmd/run_prompt.rs:197](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt.rs#L197)) and auto-injects default skills using `skills::auto_skills` ([src/cmd/run_prompt_helpers.rs:157](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt_helpers.rs#L157)).

This keyword-driven guessing mechanism is fundamentally unreliable:
1. **Tool Deprivation**: A multi-file refactor prompt classified as `simple_edit` receives only 2 of 24 available toolbox tools, stripping critical context or specialized diagnostic tools.
2. **Harmful Auto-Skills**: `skills::auto_skills` maps agent kinds to static skills ([src/skills.rs:247-272](file:///Users/mingsun/Develop/ai/ai-dispatch/src/skills.rs#L247-L272))—injecting the heavy `implementer` methodology prompt into simple read-only or research tasks, or `researcher` into Gemini calls regardless of task intent.
3. **Architectural Contradiction**: `docs/design/agent-advise-api.md` established the foundational principle that **the dispatching caller is the highest-quality model in the system** and knows task requirements, whereas `aid` cannot infer them reliably. While this principle was applied to task profiles (`--difficulty`, `--budget`, `--urgency`, `--rigor`, `--kind`) and deleting the `auto` agent, tool and skill selection was left guessing.

This proposal designs the replacement: transferring tool and skill selection entirely to explicit caller declaration and project-level configuration, replacing keyword inference with deterministic defaults and queryable advice surfaces.

---

## 2. Caller Surface Design

[FINDING] `aid` currently exposes `--skill` on `RunArgs` ([src/cmd/run_prompt_helpers.rs:154-158](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt_helpers.rs#L154-L158)), but lacks explicit `--tool` flags and relies on `auto_skills` when `--skill` is omitted.

### 2.1 CLI Flags on `aid run` and `aid advise`

The caller will control tool and skill selection via explicit CLI flags:

| Flag | Format / Values | Default when omitted | Description |
|---|---|---|---|
| `--skill` | Name(s) e.g., `--skill researcher,security` | Project default or none | Injects named methodology skill(s) |
| `--no-skill` | Boolean flag | `false` | Explicitly suppresses all skills (overrides project defaults) |
| `--tool` | Name(s) e.g., `--tool db-migrate,lint-check` | Project default or all in scope | Restricts injected toolbox tools to specified names |
| `--no-tools` | Boolean flag | `false` | Explicitly suppresses all toolbox tools |

*Note*: `--skill` and `--tool` support comma-separated values or repeated flag invocations (e.g. `--skill a --skill b`).

### 2.2 Project & Team Configuration Manifests

Projects and teams can declare default and required tool/skill sets in configuration files:

- **`.aid/project.toml`**:
```toml
[project]
id = "ai-dispatch"

[project.tools]
default = ["cargo-check", "biome-lint"] # Injected if --tool is omitted
# optional: disable_global = true       # Omit ~/.aid/tools/

[project.skills]
default = ["implementer"]               # Injected if --skill is omitted
```

- **`team.toml`** (replaces existing `tc.toolbox.auto_inject` in [src/team.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/team.rs#L35)):
```toml
[team.toolbox]
tools = ["db-migrate", "k8s-check"]
default_skills = ["ops-runbook"]
```

### 2.3 Batch Task Manifests (`[[task]]`)

In batch TOML files ([src/batch.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/batch.rs)):
```toml
[[task]]
prompt = "Refactor auth middleware"
skills = ["implementer"]
tools = ["cargo-check"]
```

### 2.4 Query Surface: `aid advise` Integration

`aid advise` ([docs/design/agent-advise-api.md:134-177](file:///Users/mingsun/Develop/ai/ai-dispatch/docs/design/agent-advise-api.md#L134-L177)) is extended to report resolved tools and skills in its human and JSON outputs so callers can preview prompt contents prior to dispatch:

```json
{
  "declared": {
    "difficulty": "complex",
    "budget": "standard",
    "skills": ["implementer"],
    "tools": ["cargo-check", "biome-lint"]
  },
  "resolved_toolbox_tools": [
    { "name": "cargo-check", "scope": "project" },
    { "name": "biome-lint", "scope": "global" }
  ],
  "resolved_skills": ["implementer"]
}
```

---

## 3. The Honest Default (When Caller Declares Nothing)

[FINDING] In `src/cmd/run_prompt.rs` (lines 180-208), when no team auto-inject list is set, `resolve_toolbox()` fetches all tools and passes them to `filter_by_task_category()`.

Removing keyword inference must be honest, predictable, and non-destructive:

### 3.1 Toolbox Tools Default: **All Resolved Tools in Scope**

- **Behavior**: When `--tool` is omitted and no `.aid/project.toml` `default` list exists, `aid` passes **100% of resolved toolbox tools** in active scope (Global `~/.aid/tools/` + Team `team/tools/` + Project `.aid/tools/`).
- **Rationale**: Omitting a tool flag must not silently hide available tools from the agent. Including text descriptions for all custom team/project tools gives the LLM full visibility without guessing what category the task belongs to.
- **Suppression**: Callers pass `--no-tools` if they want 0 toolbox tools injected.

### 3.2 Skills Default: **No Skills (Explicit Opt-In)**

- **Behavior**: When `--skill` is omitted and no `.aid/project.toml` `default` list exists, `aid` injects **0 skills**. `skills::auto_skills` is completely removed.
- **Rationale**: Skills inject substantial methodology text ("--- Methodology ---", "--- Gotchas ---") ([src/cmd/run_prompt_helpers.rs:92-95](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt_helpers.rs#L92-L95)) into `effective_prompt`, consuming token budget and forcing specific agent personas. Auto-injecting `implementer` into a simple question or `researcher` into Gemini code edits was a frequent source of prompt pollution.
- **Opt-In**: Callers pass `--skill <name>` or declare `default = ["implementer"]` in `.aid/project.toml`.

### 3.3 Execution Feedback

When running `aid run`, user-visible logs become explicit and transparent:
- `[aid] Injected 5/5 toolbox tool(s) (scope: global+project)`
- `[aid] Injected skill: implementer (declared via --skill)`
- `[aid] Skills: none`

---

## 4. Reality of Tool Injection & Agent Capabilities

[FINDING] Code inspection of [src/cmd/run_prompt.rs:205-207](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt.rs#L205-L207) and [src/toolbox.rs:214-225](file:///Users/mingsun/Develop/ai/ai-dispatch/src/toolbox.rs#L214-L225) demonstrates:
```rust
pub fn format_toolbox_instructions(tools: &[ToolMeta]) -> String {
    let mut lines = vec!["--- Team Toolbox ---".to_string()];
    lines.push("The following tools are available via bash. Use `aid tool show <name>` for full usage.".to_string());
    for tool in tools {
        lines.push(format!("  {}: {}", tool.name, tool.description));
    }
    lines.join("\n")
}
```

### 4.1 Verification of Mechanism

1. **Prompt Text Augmentation Only**: Toolbox injection formats a markdown text block appended to `effective_prompt`. It **does not** configure OS sandboxing, CLI capability flags, or native tool schemas for agent binaries.
2. **No Capability Restriction**: An investigation on 2026-08-06 confirmed that when `filter_by_task_category` filtered toolbox tools from 24 down to 2 for `simple-edit`, underlying agent CLIs (`codex`, `opencode`, `cursor`, `qwen`) retained **full, unconstrained access** to their native file read/write, git, and bash execution tools.
3. **Informational Function**: Injecting a tool description into prompt text simply informs the LLM that a helper bash script or command (`aid tool show <name>`) exists; omitting a tool description merely omits that text prompt recommendation.

---

## 5. Capability Matrix Across Agent CLIs

[FINDING] Inspection of agent adapters ([src/agent/codex.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/agent/codex.rs), [src/agent/opencode.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/agent/opencode.rs), [src/agent/claude.rs](file:///Users/mingsun/Develop/ai/ai-dispatch/src/agent/claude.rs)) shows distinct tool delivery mechanisms:

| Agent Kind | Delivery Mechanism | Native Tool Registration Support |
|---|---|---|
| `codex`, `cursor`, `qwen`, `kilo`, `gemini`, `grok`, `droid`, `oz`, `copilot` | Subprocess prompt text (`effective_prompt`) | None (relies on agent's native bash execution to run scripts) |
| `claude` | Prompt text + Optional `--mcp-config` | Full MCP protocol support via CLI flags |
| `opencode` | Prompt text + `OPENCODE_CONFIG_CONTENT` env | Native JSON tool/MCP configuration |

### 5.1 Architectural Stance

1. **Universal Baseline**: Markdown prompt text injection (`--- Team Toolbox ---` and `--- Available Scripts ---`) is the universal, lowest-common-denominator delivery mechanism supported across all 14+ agent CLIs.
2. **Adapter-Level MCP Translation (Future Extension)**: For Category B agents (`claude`, `opencode`), CLI adapters may optionally translate resolved `ToolMeta` items into native MCP server JSON configs without breaking text-based fallback for standard CLIs.

---

## 6. Migration Plan

1. **Codebase Cleanup**:
   - Delete `toolbox::filter_by_task_category` ([src/toolbox.rs:178](file:///Users/mingsun/Develop/ai/ai-dispatch/src/toolbox.rs#L178)).
   - Delete `skills::auto_skills` ([src/skills.rs:247](file:///Users/mingsun/Develop/ai/ai-dispatch/src/skills.rs#L247)).
2. **CLI Surface Update**:
   - Add `--tool` and `--no-tools` flags to `RunArgs` and `AdviseArgs`.
   - Update `effective_skills` helper in [src/cmd/run_prompt_helpers.rs:154](file:///Users/mingsun/Develop/ai/ai-dispatch/src/cmd/run_prompt_helpers.rs#L154) to evaluate `--skill` -> `.aid/project.toml` -> empty.
3. **Documentation Alignment**:
   - Update `default-skills/aid-guide` and `references/command-index.md`.
4. **Test Suite Updates**:
   - Update `run_prompt_tests.rs` and `skills/tests.rs` to reflect explicit skill/tool selection.

---

## 7. Established Gaps & Unresolved Limits

1. **LLM Adherence to Text Tools**: Text-based tool injection informs models that bash tools exist, but cannot guarantee that smaller/cheaper models will consistently invoke `aid tool show <name>` instead of writing raw shell scripts.
2. **Model-Level Tool Awareness**: CLIs running fixed prompt interfaces do not expose tool invocation events in standard JSONL streams unless wrapped by dedicated parser logic.

=== AID TASK t-00a63c9f DONE (exit 0) ===
