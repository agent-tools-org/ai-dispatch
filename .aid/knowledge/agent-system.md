# Agent System

## Adding a New Agent Adapter

1. Create `src/agent/<name>.rs` implementing the adapter (parse events, extract usage)
2. Add variant to `AgentKind` enum in `src/types.rs`
3. Add capability scores to `BASE_SCORES` matrix in `src/agent/selection_scoring.rs`
4. Wire in `src/agent/mod.rs`: `build_command()`, `parse_event()`, `agent_display_name()`
5. Add model definitions with capability scores

## Agent Selection Pipeline

```
Prompt → classifier.rs (TaskCategory + Complexity)
       → selection.rs (score each agent)
           Base score (capability matrix)
         + Team override (+N for matching category)
         + Team preferred boost (+3)
         + Budget mode adjustment (+4 cheap, -6 expensive)
         + Complexity bonus (+2 for codex/cursor on high complexity)
         + Rate limit penalty (-10 if limited)
         + History bonus/penalty (±2-3 based on success rate)
       → highest score wins
       → recommend_model() picks model tier based on complexity
```

## Prompt Assembly (run_prompt.rs)

Injection order (bottom wins for visibility):
1. Base user prompt
2. Skills (`--- Methodology ---` section)
3. Team rules (`<aid-team-rules>` tags, no filtering)
4. Team knowledge (relevance-filtered by keyword overlap)
5. Project rules (`<aid-project-rules>` tags, no filtering)
6. Project knowledge (relevance-filtered)
7. Context files (`--context`)
8. Context-from results (`--context-from`)
9. Sibling summaries (workgroup siblings' completion summaries)
10. Workspace context (`<aid-system-context>` tags)

## Event Protocol

Agents emit structured output that `watcher.rs` parses:
- `[MILESTONE] description` — progress markers shown in board/watch/TUI
- `[FINDING] content` — workgroup-scoped evidence, auto-captured
- `[MEMORY: type] content` — persistent knowledge, auto-extracted
- Usage/token data extracted per-agent from their native output format
