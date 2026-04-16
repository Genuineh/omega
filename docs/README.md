---
content_revision: 118
generation_id: gen_000037_r000118
last_verified_commit: N/A
owner: omega-team
projection_version: 37
source_doc_id: "readme:docs-readme"
status: active
updated: 2026-04-15
version: v1.0
---

# Omega Documentation Index

## Start Here

| Read this | Purpose |
|------|---------|
| `docs/TODO.md` | Open work only: current priorities, active dependencies, and current baselines |
| `docs/specs/omega-agent-spec.md` | Canonical agent contract |
| `docs/specs/omega-agent-impl-plan.md` | Entry point for the split implementation plan |
| `docs/guide/omega-dev-guide.md` | Local development workflow |
| `docs/decisions/README.md` | ADR index for durable architecture decisions |

## Current Active Work

| Track | Read these first |
|------|------------------|
| Subagent completion | `docs/TODO.md`, `docs/specs/omega-agent-impl-plan.md`, `docs/specs/omega-tui-runtime-experience.md` |
| Delivery summary contract | `docs/TODO.md`, `docs/specs/omega-task-delivery-observability.md`, `docs/specs/omega-runtime-message-pipeline.md`, `docs/specs/omega-runtime-ui-message-contract.md` |
| TUI visual polish | `docs/TODO.md`, `docs/specs/omega-tui-visual-refresh.md`, `docs/specs/omega-tui-ui-reference.md` |

## Active Reading Paths

| If you need to work on... | Read these first |
|------|------------------|
| Runtime and app boundary | `docs/specs/omega-app-package.md`, `docs/specs/omega-runtime-message-pipeline.md`, `docs/specs/omega-runtime-ui-message-contract.md`, `docs/specs/omega-task-delivery-observability.md` |
| Context assembly and session data | `docs/specs/omega-context-management.md`, `docs/specs/omega-session-resume.md`, `docs/specs/omega-project-path-layout.md`, `docs/specs/omega-operator-picker-overlay.md`, `docs/specs/omega-step-session-asset-model.md`, `docs/specs/omega-step-lifecycle-hooks.md` |
| Project identity and knowledge ownership | `docs/specs/omega-project-system.md`, `docs/specs/omega-project-path-layout.md`, `docs/specs/omega-context-management.md`, `docs/specs/omega-command-system.md`, `docs/specs/omega-tui-document-memory-supervision.md` |
| Project planning and task governance | `docs/prds/omega-project-plan-management.md`, `docs/specs/omega-project-plan-system.md`, `docs/specs/omega-project-system.md`, `docs/specs/omega-command-system.md`, `docs/specs/omega-tui-runtime-experience.md` |
| Structured document source of truth and generated docs | `docs/specs/omega-structured-document-system.md`, `docs/specs/omega-document-cli.md`, `docs/specs/omega-document-projection-versioning.md`, `docs/specs/omega-context-management.md`, `docs/specs/omega-command-system.md`, `docs/specs/omega-project-plan-system.md` |
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
- `docs/TODO.md` intentionally no longer keeps long completed-task history. If a task is not open there, treat the relevant spec and its change log as the source of truth.

## Archived But Still Useful

- `docs/archive/omega-tui-non-ui-extraction.md` keeps the first Task 15D boundary-rationalization pass.
- `docs/archive/observability-logging.md` keeps the original logging rollout plan after implementation.

## Repository Map

- `docs/specs/` is the current technical source of truth.
- `docs/guide/` is for contributor workflow and usage guidance.
- `docs/decisions/` records durable architecture decisions.
- `docs/prds/` holds formal product requirement documents for user-facing capabilities that outgrow TODO-only tracking.
- `docs/whitepapers/` is reserved for long-form rationale and vision documents.
- `docs/archive/` keeps superseded or historical material for traceability.

## Historical Notes

- The earlier `omega-repl` split plan lives in `docs/archive/omega-interaction-layer-refactor.md` and is no longer an active target.

## Change Log

- 2026-04-15: Removed the remaining `.omega/plans/` compatibility layer. `/plan` now persists only through `docs-data/tasks/`, and `docs/TODO.md` open-work projection no longer depends on dual storage or legacy import paths.
- 2026-04-15: `Task 19I ~ 19K` completed. Structured docs mutation is now CLI-only by default, and direct markdown/docs-data edits are reserved for emergency projection repair.
- 2026-04-15: `Task 19G ~ 19J` completed. `crates/omega-doc-cli` now provides foundation commands plus id-based query/mutation surface, so the remaining structured docs CLI work is narrowed to workflow enforcement, versioning, and final cutover.
- 2026-04-14: Added `docs/specs/omega-document-projection-versioning.md` and opened `Task 19H ~ 19K` to make docs mutations CLI-first, add id-based query/mutation surface, enforce workflow guidance in skills/`AGENTS.md`, and version generated docs against docs-data.
- 2026-04-14: Added `docs/specs/omega-project-plan-docs-data-convergence.md` and opened `Task 4H ~ 4K` to converge `/plan` canonical persistence onto `docs-data/`, replacing the current `.omega/plans/` vs `docs-data/` split.
- 2026-04-14: Added `docs/specs/omega-document-cli.md` and opened `Task 19G` to plan a repository-external structured docs CLI for `render` / `validate` / `extract` / `cutover` / `doctor` outside session runtime.
- 2026-04-14: Structured docs v2 cutover completed. `.claude` document skills now require structured docs v2 workflow, `docs-data/` now contains the migrated canonical records and doc task ledger, and `docs/` is regenerated from the structured source.
- 2026-04-14: Structured docs foundation moved from planning to implementation baseline: `docs-data/` layout, structured record/task/relation schema, `manage_document` structured actions, `/document render|validate|extract`, and parity-first extraction/validation are now implemented. Remaining structured docs work is limited to `.claude` skill migration and final docs cutover.
- 2026-04-14: Added `docs/specs/omega-structured-document-system.md` and indexed a dedicated reading path for docs-data/docs presentation split, omega-plan-backed document governance, and generated docs rollout.
- 2026-04-13: Added `docs/prds/omega-project-plan-management.md` and `docs/specs/omega-project-plan-system.md`, and indexed a dedicated reading path for project-scoped planning and task governance.
- 2026-04-13: Narrowed `docs/TODO.md` to open work and current baselines only, and added a dedicated `Current Active Work` section so readers can jump straight to the still-open tracks.
- 2026-04-11: Added `docs/specs/omega-project-path-layout.md` to the context/session and project-ownership reading paths so the `.omega/` vs `.omega-state/` split has a stable planning entry point.
- 2026-04-10: Added `docs/specs/omega-operator-picker-overlay.md` to the context/session, command-system, and TUI interaction reading paths so overlay-first operator selection has a stable planning entry point.
- 2026-04-10: Added `docs/specs/omega-session-resume.md` to the context/session and command-system reading paths so session restore and `/session` control-plane planning has a stable entry point.
- 2026-04-09: Added `docs/specs/omega-project-system.md` to the active reading paths so project-owned document/memory/session planning has a stable entry point.
- 2026-04-08: Added `docs/specs/omega-root-skill-routing.md` to the workflow-policy reading path so root-owned skill selection planning is indexed with scene/workflow routing.
- 2026-04-08: Added `docs/specs/omega-task-delivery-observability.md` to the runtime and TUI reading paths so task-level delivery monitoring has a single planning entry point.
- 2026-04-08: Added `docs/specs/omega-tui-ui-reference.md` to the TUI reading paths so current colors, layout ratios, row taxonomy, and control-band rules have one implementation-indexed reference.
