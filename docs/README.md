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
| `docs/specs/omega-tui-runtime-experience.md` | Specification | Cross-task TUI experience plan for future runtime-visible capabilities |
| `docs/specs/omega-tui-todo-sidebar-layout.md` | Specification | Todo/Logs split-panel layout for the TUI sidebar |
| `docs/specs/omega-tui-non-ui-extraction.md` | Specification | Planned extraction of non-UI responsibilities out of `omega-tui` |
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