---
content_revision: 122
generation_id: gen_000046_r000122
owner: omega-team
projection_version: 46
source_doc_id: "spec:omega-document-navigator"
status: draft
updated: 2026-04-24
---

# Document Navigator

## Overview

The document navigator provides TUI overlay-based interaction for `/document` commands, mirroring the `/plan` overlay pattern: a PickerOverlay for browsing, a DocumentNavigatorOverlay for detail/reading, and an overlay routing stack for back navigation.

Currently `/document` subcommands return plain text output. This spec defines how `list`, `health`, `query`, and `get` gain interactive overlays, and how structured doc relations become navigable links.

## UX Flow

```
/document list [doc_type] [status]
  -> OperatorPickerRequest [picker_id=doc-list]
     items: one per FileRecord matching filters
     title: " Documents (N) "
     item shape: path, doc_type badge, status badge, token count subtitle
     Enter -> /document open {id}             (PushOverlay -> task detail navigator)
     Ctrl-H -> /document health               (CloseOverlay)
     / -> filter
     Esc -> close

/document open <doc-id>
  -> DocumentNavigatorOverlay [navigator_id=doc-detail:{doc_id}]
     Left rail entries grouped by:
       Design group:    doc overview summary, sections
       Related group:   relations (references links to other docs)
       Context group:   metadata (file stats, indexing info)
     Right content: section content, linked doc content, or metadata body
     Tab -> toggle focus
     Ctrl-L -> pop overlay stack (return to /document list picker)
     Enter -> activate rail entry
     ↑/↓/j/k, PageUp/PageDown, Home/End -> navigate
     Esc -> close entire overlay stack

/document query <text>
  -> OperatorPickerRequest [picker_id=doc-query:{text}]
     items: one per SearchResult
     title: " Query: {text} (N) "
     item shape: path, score badge, preview subtitle
     Enter -> /document view-result {text} {id}   (PushOverlay -> navigator)
     Ctrl-L -> back (new query)
     Esc -> close

/document view-result <query> <result-path>
  -> DocumentNavigatorOverlay [navigator_id=doc-query:{query}]
     Same navigator pattern as /plan open
     Ctrl-L -> pop back to query picker
     Esc -> close

/document health
  -> DocumentNavigatorOverlay [navigator_id=doc-health]
     Left rail: health summary entry + one entry per violation
     Right content: violation detail or health summary body
     Esc -> close

/document get <doc-id>
  -> DocumentNavigatorOverlay [navigator_id=doc-detail:{doc_id}]
     Same as /document open but direct entry (no picker)
     Esc -> close
```

## Subcommand Contracts

### /document list (updated)

- **Input**: optional `[doc_type] [status]` filters
- **Output**: `OperatorPickerRequest` overlay with matching documents as items
- **Primary action Enter**: `SubmitSlashCommand` `/document open {id}` with `PushOverlay`
- **Secondary Ctrl-H**: `/document health` with `CloseOverlay`
- **Error**: no documents → "No documents found." empty state

### /document open \<doc-id\>

- **Input**: a structured doc_id (e.g. `spec:omega-plan-system`) or file path
- **Output**: `DocumentNavigatorOverlay` with doc overview, sections, relations, and metadata
- **Error**: unknown doc_id → error response, no overlay

### /document query (updated)

- **Input**: search text
- **Output**: `OperatorPickerRequest` overlay with search results as items
- **Primary action Enter**: `SubmitSlashCommand` `/document view-result {text} {id}` with `PushOverlay`
- **Error**: no results → "No results for '{text}'." empty state

### /document view-result \<query\> \<result-path\>

- **Input**: original query text and a result file path
- **Output**: `DocumentNavigatorOverlay` with search result + related docs
- **Error**: file not found → error response, no overlay

### /document health (updated)

- **Input**: none
- **Output**: `DocumentNavigatorOverlay` with health summary + violations as entries
- **Error**: no documents indexed → plain text "Run /document init first."

### /document get \<doc-id\>

- **Input**: a structured doc_id
- **Output**: `DocumentNavigatorOverlay` — same as `/document open` but without a preceding picker
- **Error**: unknown doc_id → error response, no overlay

## Picker Item Shapes

### Document list items

| Field | Value |
|-------|-------|
| `id` | doc_id or file path |
| `title` | document title or filename |
| `subtitle` | file path relative to workspace root |
| `badges` | `[doc_type]`, `[status]` |
| `preview` | first 8 lines of file if loadable |

### Query result items

| Field | Value |
|-------|-------|
| `id` | file path |
| `title` | filename component |
| `subtitle` | path · mode · score |
| `badges` | `[result]`, `[language]` |
| `preview` | search preview text |

## Navigator Entry Grouping

### /document open entries

| Group | Label | Contents |
|-------|-------|----------|
| `Design` | "Document" | overview summary, sections |
| `Related` | "Related" | inline relations (references) |
| `Context` | "Metadata" | file stats, indexing info |
| `History` | "History" | previously viewed (auto) |

### /document health entries

| Group | Label | Contents |
|-------|-------|----------|
| `Design` | "Health" | health summary overview |
| `Context` | "Issues" | individual violations |

### /document view-result entries

| Group | Label | Contents |
|-------|-------|----------|
| `Design` | "Results" | search results |
| `Context` | "Context" | related docs from snapshot |

## Resolver Strategy

`/document open` and `/document get` accept a doc_id which may be:
1. A structured doc_id like `spec:omega-plan-system` → resolve via `StructuredDocsSnapshot`
2. A file path like `docs/specs/omega-plan-system.md` → resolve via FileRecord store or structured docs snapshot
3. A partial match → search for best match in structured docs records

The resolver returns a `ResolvedDocument` containing both the `StructuredDocumentRecord` (if found) and the `FileRecord` (if indexed), plus the file content body.

## Implementation Scope

- `omega-session`: new picker/navigator request builders, updated command handlers
- `omega-tui`: no new overlay types needed — reuses `PickerOverlay`, `DocumentNavigatorOverlay`, and overlay routing stack
- `omega-document`: no changes needed — existing `FileRecord`, `StructuredDocumentRecord`, and search APIs are sufficient
