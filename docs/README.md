---
status: active
last_verified_commit: N/A
owner: omega-team
version: v1.0
---

# Omega Documentation Index

## Overview

This directory contains the working documentation set for the Omega Rust project.

## Documents

| Path | Type | Purpose |
|------|------|---------|
| `docs/TODO.md` | Tracking | Current priorities and implementation progress |
| `docs/specs/omega-agent-spec.md` | Specification | Canonical technical specification for the Omega agent |
| `docs/specs/omega-agent-impl-plan.md` | Index | Entry point for the split workspace implementation plan |
| `docs/specs/omega-agent-impl-plan/foundation-crates.md` | Plan | Tasks 1-7 for workspace setup and foundational crates |
| `docs/specs/omega-agent-impl-plan/execution-runtime-crates.md` | Plan | Tasks 8-14 for execution/runtime crates and agent foundations |
| `docs/specs/omega-agent-impl-plan/task-15-interaction-foundation.md` | Plan | Task 15 interaction boundary, workflow package, session assets, and routing foundation |
| `docs/specs/omega-agent-impl-plan/task-15-runtime-visibility.md` | Plan | Task 15 runtime contract, context evolution, diagnostics, and TUI visibility follow-ups |
| `docs/specs/omega-tui-collapsible-sidebar.md` | Specification | Collapsible sidebar shell and icon-rail interaction plan for `omega-tui` |
| `docs/specs/omega-tui-modal-keymap.md` | Specification | Modal keymap, leader routing, and `.omega` keybinding config plan for `omega-tui` |
| `docs/specs/omega-tui-overlay-popups.md` | Specification | Floating overlay / popup interaction plan for transient `omega-tui` workflows |
| `docs/specs/omega-tui-runtime-experience.md` | Specification | Active TUI-only experience plan for the richer `omega-tui` runtime path |
| `docs/specs/omega-tui-response-thinking-experience.md` | Specification | Structured Response timeline and provider-exposed thinking visibility plan |
| `docs/specs/omega-tui-todo-sidebar-layout.md` | Specification | Todo/Logs split-panel layout for the TUI sidebar |
| `docs/specs/omega-tui-non-ui-extraction.md` | Specification | Historical Task 15D extraction baseline; keep for boundary rationale, not as the latest runtime-path source of truth |
| `docs/specs/omega-app-package.md` | Specification | Implemented `omega-app` assembly package and main-entry boundary |
| `docs/specs/omega-scene-routing.md` | Specification | Planned scene-aware routing layer above execution workflows |
| `docs/specs/omega-runtime-ui-message-contract.md` | Specification | Current source of truth for `omega-tui` / `omega-session` / `omega-core` runtime-path relations and unified UI contract planning |
| `docs/specs/omega-step-session-asset-model.md` | Index | Entry point for the split step/session asset model specification |
| `docs/specs/omega-step-session-asset-model/step-assets-and-execution.md` | Specification | Step definition, session asset ownership, dynamic tool visibility, and shared execution model |
| `docs/specs/omega-step-session-asset-model/session-context-and-data-contracts.md` | Specification | Session context, summary budget, structured step input/output, and todo-driven execute contract |
| `docs/specs/omega-step-session-asset-model/routing-repair-and-diagnostics.md` | Specification | Routing convergence, structured output repair, diagnostics, migration phases, and testing guidance |
| `docs/specs/omega-step-lifecycle-hooks.md` | Specification | Planned Rust hook lifecycle, advance gate, and deterministic workflow test-harness design for hook-aware steps |
| `docs/specs/omega-tool-system-upgrade.md` | Specification | Tool system follow-up plan for structured inspection, patch-centric editing, batch orchestration, and bash demotion |
| `docs/specs/omega-workflow-package.md` | Specification | Configurable four-step workflow package and TUI phase visibility plan |
| `docs/specs/omega-client-anthropic-api-abstraction.md` | Specification | Planned Anthropic API abstraction layer for `omega-client`, with Minimax as an Anthropic-compatible provider |
| `docs/guide/omega-dev-guide.md` | Guide | Developer onboarding and local workflow guide |
| `docs/decisions/README.md` | Index | ADR index |
| `docs/decisions/001-crate-architecture.md` | ADR | Multi-crate workspace structure |
| `docs/decisions/002-rust-ratatui.md` | ADR | Rust + ratatui technology choice |
| `docs/decisions/003-trait-tool-system.md` | ADR | Trait-based tool abstraction |
| `docs/decisions/004-jsonl-message-store.md` | ADR | JSONL-backed team message storage |
| `docs/decisions/005-tracing-observability.md` | ADR | Tracing-based observability foundation |
| `docs/decisions/006-omega-tui-ui-boundary.md` | ADR | Keep `omega-tui` limited to UI responsibilities |

## Notes

- `docs/specs/` contains the current technical source of truth.
- `docs/guide/` contains contributor-facing usage documentation.
- `docs/decisions/` records architecture choices and tradeoffs.
- Step lifecycle note: `omega-step-session-asset-model.md` remains the entrypoint for the current step/session asset baseline; the split child docs under `docs/specs/omega-step-session-asset-model/` now hold the detailed asset, context, routing, repair, and diagnostics contracts, while `omega-step-lifecycle-hooks.md` captures the planned hook-driven lifecycle and advance-gate direction.
- Interaction-layer note: use `omega-runtime-ui-message-contract.md` for the current runtime path and `omega-app-package.md` for the implemented app-entry boundary; treat `omega-tui-non-ui-extraction.md` as the completed Task 15D baseline rather than the newest planning source.
- Historical note: the earlier `omega-repl` split plan has been archived to `docs/archive/omega-interaction-layer-refactor.md`; it is no longer an active architecture target.