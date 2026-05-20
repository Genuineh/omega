---
content_revision: 122
generation_id: gen_000046_r000122
owner: omega-team
projection_version: 46
source_doc_id: "spec:omega-plan-task-detail-navigator"
status: active
updated: 2026-04-24
---

# Plan Task Detail Navigator

## Overview

The plan task detail navigator provides a two-level TUI navigation path from the task list to task detail with linked artifact content. Level 1 uses `OperatorPickerRequest` for the task list. Level 2 uses `DocumentNavigatorOverlay` (dual-pane: rail + content) for the full task detail view.

The primary navigation path uses an overlay routing stack: `/plan list` Enter pushes the picker onto the stack and opens the task detail navigator. Ctrl-L pops the stack to return to the picker with full state preserved. Esc closes the entire stack.

`/plan` subcommands: `/plan list`, `/plan open`, `/plan links`, `/plan open-link`, `/plan view-file`.

## UX Flow

```
/plan list
  -> OperatorPickerRequest [picker_id=plan-list]
     ↑/↓ navigate tasks
     Enter -> /plan open <id>        (SubmitSlashCommand, PushOverlay)
     Ctrl-S -> /plan select <id>     (CloseOverlay)
     / -> filter
     Esc -> close

/plan open <task-id>
  -> DocumentNavigatorOverlay [navigator_id=plan-detail:{task-id}]
     Left rail: entries grouped by Design / Related / Context
       Design group:         task overview summary, design_links, presentation_links
       Related group:        implementation_links
       Context group:        recent logs
     Right content: summary body or file/doc/log body with breadcrumbs
     Tab -> toggle focus between rail and content
     Ctrl-L -> pop overlay stack (return to /plan list picker)
     Enter -> activate rail entry
     ↑/↓/j/k, PageUp/PageDown, Home/End -> navigate
     Esc -> close entire overlay stack

/plan links <id>          (secondary entry, not default user path)
  -> OperatorPickerRequest [picker_id=plan-links:{id}]
     items: design_links + impl_links + presentation_links + recent logs
     Enter -> /plan open-link <task-id> <target-id>
     Ctrl-L -> /plan list
     Esc -> close

/plan open-link <task-id> <target-id>
  -> DocumentNavigatorOverlay [navigator_id=plan-links:{task-id}]
     Same keyboard controls as /plan open
     Ctrl-L -> back
     Esc -> close

/plan view-file <path>
  -> DocumentNavigatorOverlay [navigator_id=plan-view-file:{path}]
     Single-entry navigator for direct file viewing
     Esc -> close
```

## Overlay Routing Stack

`App` maintains an `overlay_stack: Vec<OverlayState>` alongside `overlay: Option<OverlayState>`.

- `push_overlay(new_state)`: saves current overlay to stack, sets new overlay as active
- `pop_overlay()`: discards current overlay, restores previous from stack; returns false if stack was empty (falls back to close)
- `close_overlay()`: discards current overlay and clears entire stack, restores origin_panel

When `/plan list` Enter triggers `PushOverlay` behavior, the TUI pushes the picker onto the stack before submitting the `/plan open {id}` command. When the task detail navigator opens, Ctrl-L calls `pop_overlay()` to restore the exact picker state (selected index, filter query, scroll position).

## Subcommand Contracts

### /plan open \<task-id\>

- **Input**: a `TASK-XXXX` id
- **Output**: `DocumentNavigatorOverlay` with task overview, design/impl/presentation links, and recent logs
- **Default active entry**: task overview summary entry
- **Error**: unknown id → error response, no overlay

### /plan links \<task-id\>

- **Input**: a `TASK-XXXX` id
- **Output**: `OperatorPickerRequest` overlay, items grouped by kind (`design`, `implementation`, `presentation`, `log`)
- **Error**: unknown id → error response, no overlay

### /plan open-link \<task-id\> \<target-id\>

- **Input**: task id and target artifact id (file path or `log-entry:<seq>`)
- **Output**: `DocumentNavigatorOverlay` with all linked artifacts for the task, focused on the selected target
- **Error**: unknown task or target → error response, no overlay

### /plan view-file \<path\>

- **Input**: workspace-relative path
- **Output**: `DocumentNavigatorOverlay` with the file content
- **Security**: path must resolve inside workspace root; path traversal (`../`) is rejected
- **Error**: file not found → error response, no overlay

### /plan list (updated)

- Primary action `Enter`: `SubmitSlashCommand` `/plan open {id}` with `PushOverlay`
- Secondary `Ctrl-S`: `/plan select {id}` with `CloseOverlay`

## Task Detail Navigator Entries

The `build_task_detail_navigator_request` builds the following entries:

| Group | Entry | Kind | Body |
|-------|-------|------|------|
| Design | Task Overview | Summary | title, priority, status, requirement, dependencies, acceptance |
| Design | design_links | Document/File | file content preview |
| Design | presentation_links | Document/File | file content preview |
| Related | implementation_links | File | file content preview |
| Context | recent_logs | Log | log entry detail |
| History | previously viewed | (auto) | (auto) |

## Navigator Grouping

The `DocumentNavigatorOverlay` rail groups entries by `DocumentNavigatorGroup`:

| Group | Label | Contents |
|-------|-------|----------|
| `Design` | "Design" | task overview, design_links, presentation_links |
| `Related` | "Related" | implementation_links |
| `Context` | "Context" | recent logs |
| `History` | "History" | previously viewed entries (auto-managed) |

## Security

`/plan view-file` canonicalizes the input path relative to the workspace root. Any path that resolves outside the root is rejected with an error response. Symlinks escaping the root are also rejected. No shell expansion is applied to the path argument.

## Implementation Scope

Command logic lives in `crates/omega-session/src/lib.rs`. Overlay rendering lives in `crates/omega-tui/src/render/overlay.rs`. Event handling lives in `crates/omega-tui/src/event/overlay_handlers.rs`. Stack management lives in `crates/omega-tui/src/app.rs`.

- `omega-plan`: provides `PlannedTask` including `design_links`, `implementation_links`, `presentation_links`, and work logs
- `omega-session`: builds `OperatorPickerRequest` and `DocumentNavigatorRequest` from task context; defines `OperatorPickerOverlayBehavior::PushOverlay`
- `omega-tui`: renders `PickerOverlay` and `DocumentNavigatorOverlay`, manages overlay routing stack and selection memory
