---
content_revision: 174
generation_id: gen_000087_r000174
language: en
last_verified_commit: 152deb1818837dc5c3e7575c7010dc965eef2c38
owner: omega-team
projection_version: 87
source_doc_id: "spec:omega-plan-task-detail-navigator"
source_path: docs/specs/omega-plan-task-detail-navigator.md
status: draft
updated: 2026-06-03
---

# Plan Task Detail Navigator

## Overview

The plan task detail navigator provides a three-level TUI navigation path from the task list to linked artifact content. It reuses the existing `OperatorPickerRequest` overlay machinery and adds two new `/plan` subcommands: `/plan links` and `/plan view-file`.

## UX Flow

```
/plan list
  -> OperatorPickerRequest [picker_id=plan-list]
     ↑/↓ navigate tasks
     Enter -> /plan links <id>   (SubmitSlashCommand, CloseOverlay)
     Ctrl-S -> /plan select <id> (unchanged)
     Esc -> close

/plan links <id>
  -> OperatorPickerRequest [picker_id=plan-links:{id}]
     title: "  TASK-XXXX: {title}  "
     items: design_links + impl_links + presentation_links + recent logs
     Enter -> /plan view-file <path>   (SubmitSlashCommand, CloseOverlay)
     Ctrl-L -> /plan list              (back)
     Esc -> close

/plan view-file <path>
  -> DetailOverlay with stripped file content lines
     ↑/↓ scroll     Esc -> close
```

## Subcommand Contracts

### /plan links \<task-id\>

- **Input**: a `TASK-XXXX` id
- **Output**: `OperatorPickerRequest` overlay, items grouped by kind (`design`, `implementation`, `doc`, `log`)
- **Error**: unknown id → error response, no overlay

### /plan view-file \<path\>

- **Input**: workspace-relative path
- **Output**: `DetailOverlay` with stripped file content lines
- **Security**: path must resolve inside workspace root; path traversal (`../`) is rejected
- **Error**: file not found → error response, no overlay

### /plan list (updated)

- Primary action `Enter`: `SubmitSlashCommand` `/plan links {id}` with `CloseOverlay`
- Secondary `Ctrl-S`: `/plan select {id}` (unchanged)

## Link Item Shape

Each file link item in the `/plan links` picker:

| Field | Value |
|-------|-------|
| `id` | file path |
| `title` | filename component |
| `subtitle` | path relative to workspace root |
| `badges` | `[kind]` e.g. `spec`, `guide`, `impl` |
| `preview` | first 8 lines of file if loadable |
| primary `Enter` | `/plan view-file <path>` (CloseOverlay) |
| secondary `Ctrl-L` | `/plan list` |

Work log items:

| Field | Value |
|-------|-------|
| `id` | `log-entry:<seq>` |
| `title` | log summary text |
| `badges` | `[log]` |
| primary `Enter` | `OpenDetail` (shows full log entry in DetailOverlay) |

## Security

`/plan view-file` canonicalizes the input path relative to the workspace root. Any path that resolves outside the root is rejected with an error response. Symlinks escaping the root are also rejected. No shell expansion is applied to the path argument.

## Implementation Scope

All new command logic lives in `crates/omega-session/src/lib.rs`. No new TUI overlay types are required. The existing `OperatorPickerRequest` / `DetailOverlay` pipeline handles all three navigation levels.

- `omega-plan`: provides `PlannedTask` including `design_links`, `implementation_links`, and work logs
- `omega-document`: provides `StructuredDocTask` for `presentation_links` cross-reference lookup

## Implementation Note


The `omega-project-layout`, `omega-memory`, `omega-document`, and `omega-doc-cli` crates referenced in this spec moved to the `omega-hpc/` sub-workspace on 2026-06-02 and are now `omega-hpc-paths`, `omega-hpc-memory`, `omega-hpc-document`, and `omega-hpc-doc-cli` respectively. Public type and binary names are unchanged. See [`docs/specs/omega-hpc-extraction.md`](omega-hpc-extraction.md) for the full mapping and [`docs/decisions/007-omega-hpc-extraction.md`](../decisions/007-omega-hpc-extraction.md) for the architecture decision.
