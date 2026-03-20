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
| `docs/specs/omega-agent-impl-plan.md` | Plan | Step-by-step implementation plan for the workspace |
| `docs/specs/omega-tui-collapsible-sidebar.md` | Specification | Collapsible sidebar shell and icon-rail interaction plan for `omega-tui` |
| `docs/specs/omega-tui-modal-keymap.md` | Specification | Modal keymap, leader routing, and `.omega` keybinding config plan for `omega-tui` |
| `docs/specs/omega-tui-overlay-popups.md` | Specification | Floating overlay / popup interaction plan for transient `omega-tui` workflows |
| `docs/specs/omega-tui-runtime-experience.md` | Specification | Active TUI-only experience plan for the richer `omega-tui` runtime path |
| `docs/specs/omega-tui-todo-sidebar-layout.md` | Specification | Todo/Logs split-panel layout for the TUI sidebar |
| `docs/specs/omega-tui-non-ui-extraction.md` | Specification | Historical Task 15D extraction baseline; keep for boundary rationale, not as the latest runtime-path source of truth |
| `docs/specs/omega-runtime-ui-message-contract.md` | Specification | Current source of truth for `omega-tui` / `omega-session` / `omega-core` runtime-path relations and unified UI contract planning |
| `docs/specs/omega-step-session-asset-model.md` | Specification | Planned step model and session asset management boundary for workflow execution |
| `docs/specs/omega-workflow-package.md` | Specification | Configurable four-step workflow package and TUI phase visibility plan |
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
- Interaction-layer note: use `omega-runtime-ui-message-contract.md` for the current TUI/session/core runtime path; treat `omega-tui-non-ui-extraction.md` as the completed Task 15D baseline rather than the newest planning source.
- Historical note: the earlier `omega-repl` split plan has been archived to `docs/archive/omega-interaction-layer-refactor.md`; it is no longer an active architecture target.