# ai-dispatch — Project Knowledge

- [Architecture](architecture.md) — Module layout, data flow, key types, prompt assembly order
- [Lifecycle Refactor Roadmap](lifecycle-refactor-roadmap.md) — Phase plan for decomposing task completion logic into smaller modules
- [Lifecycle Refactor Design](lifecycle-refactor-design.md) — Target state, boundaries, and status model changes for the run lifecycle
- [Lifecycle Cross-Audit Plan](lifecycle-cross-audit-plan.md) — Review gates, test matrix, and audit workflow for each refactor slice
- [Lifecycle Phase 1 Audit](lifecycle-phase1-audit.md) — Audit result for the module wiring slice
- [Lifecycle Phase 2 Audit](lifecycle-phase2-audit.md) — Audit result for the delivery-assessment slice
- [Lifecycle Phase 3 Audit](lifecycle-phase3-audit.md) — Audit result for persisted delivery assessment and legacy status migration
- [Coding Conventions](coding-conventions.md) — File structure, Rust patterns, testing, CLI command pattern, how to add commands
- [Agent System](agent-system.md) — Selection pipeline, prompt injection order, event protocol, how to add agents
- [Build & Release](build-and-release.md) — Build commands, release checklist, website deploy, macOS signing
- [Common Pitfalls](common-pitfalls.md) — Agent behavior issues, test isolation, worktree edge cases, SQLite, RunArgs
