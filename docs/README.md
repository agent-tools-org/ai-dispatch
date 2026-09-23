# Documentation map

Start with the [2026-09-22 takeover inventory](project-status-2026-09-22.md) for the
codebase map, source-backed risks, and verification limits; use the [roadmap](roadmap.md)
for current sequencing and acceptance gates.

| Need | Source |
| --- | --- |
| Product introduction and examples | [Root README](../README.md) |
| Authoritative operating commands and contracts | [AID guide](../default-skills/aid-guide/SKILL.md) and its references |
| Current implementation map | [Project inventory](project-status-2026-09-22.md), [internal architecture](../.aid/knowledge/architecture.md) |
| Contributor and release rules | [CLAUDE.md](../CLAUDE.md) |
| Latest custody test evidence | [2026-09-23 validation](validation-custody-2026-09-23.md) |
| Latest target-budget test evidence | [2026-09-23 budget validation](validation-budget-2026-09-23.md) |
| Shipped changes | [CHANGELOG](../CHANGELOG.md) |
| Client/API design | [Client API](design/client-api.md), [XcodeGen targets](../client/project.yml) |
| Artifact ownership | [Acceptance and worktree lifecycle](design/principal-acceptance-worktree-lifecycle.md) |
| Result semantics | [Task success contract](design-task-success-contract.md) |
| Historical architecture findings | [July audit](audit-architecture-2026-07.md) |
| Historical release candidate evidence | [August handoff](audit-handoff-2026-08-23.md) |
| UX candidates needing re-triage | [UX debt](ux-debt.md) |

Files named `audit-*`, `investigation-*`, `triage-*`, and dated research reports preserve
incident evidence at their recorded baseline. Design documents describe intent and may
not describe every current code path. Verify implementation and regression coverage
before promoting an old finding into the active queue or treating it as resolved.

Keep current priorities in one place (`roadmap.md`); link historical evidence rather
than rewriting old audit results. Public command/flag/lifecycle/config changes must
also update the release-managed AID guide in the same implementation change.
