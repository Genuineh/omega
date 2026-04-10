---
status: active
last_verified_commit: N/A
owner: omega-team
version: v1.0
---

# Omega Documentation Index

## Start Here

| Read this | Purpose |
|------|---------|
| `docs/TODO.md` | Current priorities, recent maintenance, and milestone status |
| `docs/specs/omega-agent-spec.md` | Canonical agent contract |
| `docs/specs/omega-agent-impl-plan.md` | Entry point for the split implementation plan |
| `docs/guide/omega-dev-guide.md` | Local development workflow |
| `docs/decisions/README.md` | ADR index for durable architecture decisions |

## Active Reading Paths

| If you need to work on... | Read these first |
|------|------------------|
| Runtime and app boundary | `docs/specs/omega-app-package.md`, `docs/specs/omega-runtime-message-pipeline.md`, `docs/specs/omega-runtime-ui-message-contract.md`, `docs/specs/omega-task-delivery-observability.md` |
| Context assembly and session data | `docs/specs/omega-context-management.md`, `docs/specs/omega-session-resume.md`, `docs/specs/omega-operator-picker-overlay.md`, `docs/specs/omega-step-session-asset-model.md`, `docs/specs/omega-step-lifecycle-hooks.md` |
| Project identity and knowledge ownership | `docs/specs/omega-project-system.md`, `docs/specs/omega-context-management.md`, `docs/specs/omega-command-system.md`, `docs/specs/omega-tui-document-memory-supervision.md` |
| Knowledge evolution and long-term project memory | `docs/specs/omega-knowledge-evolution.md`, `docs/specs/omega-context-management.md`, `docs/specs/omega-tui-document-memory-supervision.md` |
| Tools and workflow policy | `docs/specs/omega-tool-system-upgrade.md`, `docs/specs/omega-tool-prompt-optimization.md`, `docs/specs/omega-workflow-package.md`, `docs/specs/omega-scene-routing.md`, `docs/specs/omega-root-skill-routing.md` |
| Command system and document knowledge | `docs/specs/omega-command-system.md`, `docs/specs/omega-session-resume.md`, `docs/specs/omega-operator-picker-overlay.md`, `docs/specs/omega-project-system.md`, `docs/specs/omega-context-management.md`, `docs/specs/omega-tui-document-memory-supervision.md` |
| Client/provider integration | `docs/specs/omega-client-anthropic-api-abstraction.md`, `docs/specs/omega-deterministic-test-seams.md` |
| TUI shell and interaction model | `docs/specs/omega-tui-runtime-experience.md`, `docs/specs/omega-tui-collapsible-sidebar.md`, `docs/specs/omega-tui-document-memory-supervision.md`, `docs/specs/omega-tui-modal-keymap.md`, `docs/specs/omega-tui-overlay-popups.md`, `docs/specs/omega-operator-picker-overlay.md`, `docs/specs/omega-task-delivery-observability.md` |
| TUI rendering and message presentation | `docs/specs/omega-tui-ui-reference.md`, `docs/specs/omega-tui-message-cards.md`, `docs/specs/omega-tui-visual-refresh.md`, `docs/specs/omega-tui-message-display-polish.md`, `docs/specs/omega-tui-response-thinking-experience.md`, `docs/specs/omega-tui-step-subflow-visibility.md`, `docs/specs/omega-tui-step-tool-thinking-refinement.md` |
| TUI layout follow-ups | `docs/specs/omega-tui-ui-reference.md`, `docs/specs/omega-tui-input-status-layout.md`, `docs/specs/omega-tui-todo-sidebar-layout.md` |

## How To Read The Split Specs

- `docs/specs/omega-agent-impl-plan.md` is the index for the implementation-plan subtree. Read it first, then jump into the child plan that matches the current task.
- `docs/specs/omega-step-session-asset-model.md` is the index for the step/session asset subtree. Use the child docs only when you need detailed contracts for assets, context/data flow, or routing/diagnostics.

## Archived But Still Useful

- `docs/archive/omega-tui-non-ui-extraction.md` keeps the first Task 15D boundary-rationalization pass.
- `docs/archive/observability-logging.md` keeps the original logging rollout plan after implementation.

## Repository Map

- `docs/specs/` is the current technical source of truth.
- `docs/guide/` is for contributor workflow and usage guidance.
- `docs/decisions/` records durable architecture decisions.
- `docs/prds/` is reserved for formal product requirement documents (currently tracked in TODO.md).
- `docs/whitepapers/` is reserved for long-form rationale and vision documents.
- `docs/archive/` keeps superseded or historical material for traceability.

## Historical Notes

- The earlier `omega-repl` split plan lives in `docs/archive/omega-interaction-layer-refactor.md` and is no longer an active target.

## Change Log

- 2026-04-10: Added `docs/specs/omega-operator-picker-overlay.md` to the context/session, command-system, and TUI interaction reading paths so overlay-first operator selection has a stable planning entry point.
- 2026-04-10: Added `docs/specs/omega-session-resume.md` to the context/session and command-system reading paths so session restore and `/session` control-plane planning has a stable entry point.
- 2026-04-09: Added `docs/specs/omega-project-system.md` to the active reading paths so project-owned document/memory/session planning has a stable entry point.
- 2026-04-08: Added `docs/specs/omega-root-skill-routing.md` to the workflow-policy reading path so root-owned skill selection planning is indexed with scene/workflow routing.
- 2026-04-08: Added `docs/specs/omega-task-delivery-observability.md` to the runtime and TUI reading paths so task-level delivery monitoring has a single planning entry point.
- 2026-04-08: Added `docs/specs/omega-tui-ui-reference.md` to the TUI reading paths so current colors, layout ratios, row taxonomy, and control-band rules have one implementation-indexed reference.