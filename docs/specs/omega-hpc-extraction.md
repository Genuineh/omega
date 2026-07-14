---
content_revision: 174
generation_id: gen_000087_r000174
language: en
last_verified_commit: uncommitted+df30a76a3dffa2afb72515ed3f0a041ac0e053fd
projection_version: 87
source_doc_id: "spec:docs-specs-omega-hpc-extraction"
source_path: docs/specs/omega-hpc-extraction.md
---

# omega-hpc Extraction Spec

## Overview

Status: active
Owners: omega-hpc working group
Last updated: 2026-06-02

## 1. Purpose

Extract the memory + document + doc-cli + project-layout stack from the main
omega workspace into a sibling sub-workspace named `omega-hpc/`. The sub-workspace
is the seed of an independent, embeddable persistent-context package that
downstream projects (including omega itself) can consume without dragging in the
larger AI / TUI / workflow stack.

## 2. Scope

In scope (members of the new sub-workspace, crate names re-prefixed):

- `omega-hpc-paths` (was `omega-project-layout`) — path constants, directory
  layout, store-ignore parsing.
- `omega-hpc-memory` (was `omega-memory`) — file store, keyword/vector index,
  hybrid search, todo store, doc record projections.
- `omega-hpc-document` (was `omega-document`) — structured doc system,
  manifest + render state, projection rendering, `TaskProjection` DTO, and the
  canonical `TodoItem` / `TodoStatus` / `TodoManager` data types.
- `omega-hpc-doc-cli` (was `omega-doc-cli`) — `omega-doc` binary.

Out of scope (stays in the main omega workspace as a consumer):

- `omega-todo` (tool handler layer) — now a thin re-export of the data types
  defined in `omega-hpc-document`.
- `omega-plan` — owns the rich `PlannedTask` / `PlannedTaskStatus` /
  `TaskPriority` types and writes the canonical `docs-data/tasks/project-tasks.jsonl`
  consumed by `omega-hpc-document` via the `TaskProjection` DTO.
- `omega-context` — facade that wires `omega-hpc-document` together with the
  rest of the workspace; adapter coupling to `omega-client` / `omega-tools` /
  `omega-workflow` keeps it in main.
- `omega-core`, `omega-session`, `omega-project`, `omega-app`, `omega-tui`,
  `omega-theme`, `omega-keymap`, `omega-hooks`, `omega-workflow`,
  `omega-benchmark` — all become path-dep consumers of the re-prefixed crates.

## 3. Public API stability

Public type and binary names are preserved so downstream call sites only need
to update `use` paths and `package.name` references:

- `OmegaDocument`, `DocumentOp`, `DocumentMutationMode`, `OmegaContextFacade`,
  `OmegaProjectLayout`, `DocType`, `FileType`, `FileStatus`, `FileRecord`,
  `Chunk`, `SearchQuery`, `SearchMode`, `ScanResult`, `TodoSnapshot`,
  `TodoOp`, `TodoOpResult` — names unchanged.
- `TaskProjection` is the new public DTO (added during decoupling, see §4).
- `omega-doc` binary name and `run` entry point unchanged.
- Crate names gain the `omega-hpc-` prefix; the `omega-` prefix is reserved for
  the main workspace.

## 4. Dependency inversion (the hard prerequisite)

Before the physical move, two dependency edges must be inverted because the
target sub-workspace cannot depend on `omega-todo` or `omega-plan` (both stay in
main).

### 4.1 `omega-document` → `omega-todo` removed

`TodoItem`, `TodoStatus`, `TodoManager`, `SharedTodoManager`, `MAX_TODO_ITEMS`,
and `TODO_REMINDER` are pure data + rendering logic. They now live in
`omega-hpc-document::todo` (new `todo` module) and are re-exported from
`omega_todo` for backwards compatibility.

`omega-todo` keeps its tool-handler surface (`TodoToolHandler`,
`TodoWriteHandler`, `TodoReadHandler`) and gains a path-dep on
`omega-hpc-document`.

### 4.2 `omega-document` → `omega-plan` removed

`render_document` previously accepted `&[PlannedTask]` from `omega-plan` and
called `ProjectPlanStore::open_or_scaffold` to load tasks. Both edges are
removed.

`omega-hpc-document` defines a new `TaskProjection` DTO with exactly the fields
needed for rendering (`id`, `title`, `status`, `priority`, `doc_scope`,
`summary`, `depends_on`). The DTO is deserialized directly from
`docs-data/tasks/project-tasks.jsonl`, which `omega-plan` continues to own and
write. The JSONL schema is the cross-crate contract.

`render_document` now takes `&[TaskProjection]`; the status filter uses string
comparison against the closure set `done | archived | completed`. `doc_scope`
matching is unchanged.

### 4.3 Test fixtures

The `todo_projection_renders_project_tasks_with_todo_scope` test in
`omega-hpc-document` no longer imports `omega-plan`; it writes the
`project-tasks.jsonl` fixture directly via `serde_json::to_string(&TaskProjection)`.
This decouples the rendering tests from the `PlannedTask` schema evolution
track.

## 5. Physical layout

```
omega/
├── Cargo.toml                # main workspace, exclude = ["omega-hpc"]
├── crates/                   # main workspace members (32 - 4 = 28 after extraction)
└── omega-hpc/                # new sub-workspace
    ├── Cargo.toml            # sub-workspace root
    ├── README.md
    ├── LICENSE
    └── crates/
        ├── omega-hpc-paths/
        ├── omega-hpc-memory/
        ├── omega-hpc-document/
        └── omega-hpc-doc-cli/
```

The root `Cargo.toml` adds `exclude = ["omega-hpc"]` so cargo does not try to
discover the sub-workspace from the main root. Each sub-workspace can be
inspected and built in isolation with `cargo -C omega-hpc …` or
`cargo --manifest-path omega-hpc/Cargo.toml …`.

## 6. Build, test, lint

- `cargo fmt --all` runs in both workspaces.
- `cargo check` and `cargo test` run in both.
- `cargo test-document-backend` and `cargo test-document-commands` aliases
  update to use the re-prefixed package names; the `omega-context/document-backend`
  feature flag references `dep:omega-hpc-document`.
- `omega-doc doctor --root .`, `omega-doc validate --root .`, and
  `omega-doc render <doc-id> --root .` should produce byte-identical output
  before and after the move.

## 7. Documentation updates

- `AGENTS.md` Project Structure section gains an `omega-hpc/` entry.
- `docs/specs/omega-context-management.md`,
  `docs/specs/omega-structured-document-system.md`, and
  `docs/specs/omega-document-cli.md` (if present) gain path/crate updates
  pointing at the re-prefixed names.
- `docs/decisions/007-omega-hpc-extraction.md` records the architecture
  decision: why the extraction, why the `omega-hpc-` prefix, why the
  dependency-inversion prerequisite.

## 8. Migration sequencing (Tasks 38 / 38A / 38B…38I)

1. **38** — Author this spec (done before 38A so the contract is locked).
2. **38A** — Decouple `omega-document` from `omega-todo` and `omega-plan`
   (sink `TodoItem` into `omega-hpc-document`, define `TaskProjection`).
   Hard prerequisite for the physical move.
3. **38B** — Scaffold `omega-hpc/` sub-workspace with shared
   `workspace.package` and `workspace.dependencies`.
4. **38C** — Move `omega-project-layout` to `omega-hpc-paths`.
5. **38D** — Move `omega-memory` to `omega-hpc-memory`.
6. **38E** — Move `omega-document` to `omega-hpc-document`.
7. **38F** — Move `omega-doc-cli` to `omega-hpc-doc-cli`.
8. **38G** — Rewire the main workspace: remove 4 members from `members`,
   update all `path = "../omega-X"` to the new locations, rewrite
   `use omega_X::*` → `use omega_hpc_X::*` in 13+ downstream crates, update
   `.cargo/config.toml` aliases, update `omega-context` feature flag.
9. **38H** — Update `AGENTS.md`, refresh dependent specs, write ADR-007.
10. **38I** — Run `cargo fmt --all` + `cargo test` in both workspaces, run
    the `test-document-backend` / `test-document-commands` aliases, run
    `omega-doc doctor|validate|render`, diff `docs/TODO.md` against the
    pre-move render to confirm byte-identical output.

## 9. Out of scope (future work)

- Optimizing the hpc crates for a standalone release profile (`opt-level = "s"`,
  LTO, `codegen-units = 1`, `strip = true`, `panic = "abort"`) — left for the
  independent-project phase.
- Publishing hpc crates to crates.io — only after the standalone project has
  its own version + changelog policy.
- Splitting `omega-hpc-memory` further (e.g. separating file store, keyword
  index, vector index) — not required by the current extraction.
