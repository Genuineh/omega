---
content_revision: 174
generation_id: gen_000087_r000174
language: en
last_verified_commit: uncommitted+3cd88c08a0808d360a3f4a0cba8b78b5d8378675
projection_version: 87
source_doc_id: "adr:docs-decisions-007-omega-hpc-extraction"
source_path: docs/decisions/007-omega-hpc-extraction.md
---

# ADR-007: Extract memory + document + doc-cli + project-layout into an `omega-hpc` sub-workspace

## Overview

Status: accepted (2026-06-02)
Owners: omega-hpc working group
Related: `docs/specs/omega-hpc-extraction.md`

## Context

The main omega workspace couples the persistent-context stack
(`omega-project-layout`, `omega-memory`, `omega-document`, `omega-doc-cli`) with
the larger AI / TUI / workflow stack (`omega-client`, `omega-tui`, `omega-app`,
`omega-workflow`, etc.). Two consequences followed:

1. **Cannot be embedded outside omega.** A consumer that wants structured
   documents + memory + the `omega-doc` CLI must also depend on
   `omega-tools`, `omega-tui`, `omega-context`, `omega-core`, `omega-hooks`,
   etc., even if they only want the persistent-context surface.
2. **Coupling drag on iteration speed.** The persistent-context crates have
   no in-tree consumer outside omega itself, yet they share a single Cargo
   workspace with crates that pull heavy deps (e.g. `lance-encoding` with its
   `protoc` build requirement, `lancedb`, `fastembed`, `tantivy`). A change
   to one crate rebuilds the world, and build times punish iteration.

Additionally, the existing dependency graph had two cross-workspace edges
that had to be inverted before any physical move:

- `omega-document` depended on `omega-todo` (for `TodoItem` / `TodoManager`).
- `omega-document` depended on `omega-plan` (for `PlannedTask` and
  `ProjectPlanStore::open_or_scaffold` to load tasks for the "## Project
  Tasks" section of the rendered TODO).

A standalone hpc package cannot depend on `omega-todo` or `omega-plan` —
those stay in the main omega workspace because they are tightly coupled to
tool-handler wiring (`omega-tools`) and project-plan business logic
respectively.

## Decision

1. **Create a sibling sub-workspace** at `omega-hpc/` with its own
   `Cargo.toml`, `README.md`, `LICENSE`, and `.gitignore`. The main
   workspace's root `Cargo.toml` adds `exclude = ["omega-hpc"]` so cargo
   does not try to discover the sub-workspace from the main root.
2. **Re-prefix the moved crates** with `omega-hpc-`:
   - `omega-project-layout` → `omega-hpc-paths`
   - `omega-memory` → `omega-hpc-memory`
   - `omega-document` → `omega-hpc-document`
   - `omega-doc-cli` → `omega-hpc-doc-cli`
3. **Preserve public API surface.** Public type names (`OmegaDocument`,
   `OmegaProjectLayout`, `TodoItem`, `TodoStatus`, `TodoManager`, `DocType`,
   `SearchMode`, etc.) and the `omega-doc` binary name are unchanged.
   Downstream call sites only need `use` path and `package.name` updates.
4. **Invert the dependency edges before the move.**
   - `TodoItem` / `TodoStatus` / `TodoManager` / `SharedTodoManager` /
     `MAX_TODO_ITEMS` / `TODO_REMINDER` are pure data + rendering logic.
     They now live in `omega-hpc-document::todo` and are re-exported from
     `omega-todo` for backwards compatibility. `omega-todo` keeps its
     tool-handler surface and gains a path-dep on `omega-hpc-document`.
   - The "## Project Tasks" renderer reads the canonical
     `docs-data/tasks/project-tasks.jsonl` via a new `TaskProjection` DTO
     defined in `omega-hpc-document::structured_docs`. `omega-plan` keeps
     owning `PlannedTask` and writing the JSONL; the JSONL schema is the
     cross-crate contract. `render_document` no longer accepts
     `&[PlannedTask]`; it takes `&[TaskProjection]`.
5. **Out of scope for this extraction.** Optimizing the hpc crates for
   standalone release (smaller binaries, LTO, `opt-level = "s"`,
   `panic = "abort"`) is a follow-up. So is publishing the hpc crates
   independently to crates.io.

## Alternatives considered

- **Move the crates into a separate git repo.** Rejected. The hpc stack
  shares the same `docs-data/` schema and tests as the rest of omega.
  A monorepo keeps version alignment and CI cost low; a sub-workspace is
  the minimum split that gives independent build/test isolation.
- **Move omega-todo and omega-plan into the hpc sub-workspace too.**
  Rejected. `omega-todo` is the tool-handler surface for the assistant
  loop and depends on `omega-tools`; `omega-plan` has rich task domain
  types and business logic that has nothing to do with the persistent
  document stack. Both stay in the main workspace as consumers.
- **Keep `omega-document` in the main workspace but extract only
  `omega-memory` and `omega-project-layout`.** Rejected. The point of
  the extraction is the unit of "persistent context" — file store +
  keyword/vector index + structured doc system + projection CLI. Splitting
  it would leave the structured-doc system stranded, and `omega-doc` is
  the canonical external entry point so it belongs with the rest.
- **Define a `TaskProjectionSource` trait in `omega-hpc-document` and
  have `omega-plan` implement it.** Rejected for v1. A trait-based
  inversion adds a layer of indirection that buys us nothing yet:
  `omega-hpc-document` only needs to read the JSONL. The JSONL schema is
  a simpler, more stable contract. We can introduce the trait later if
  multiple task sources appear.

## Consequences

### Positive

- The hpc stack can be inspected and built in isolation with
  `cargo -C omega-hpc …` or
  `cargo --manifest-path omega-hpc/Cargo.toml …`.
- Iteration on the persistent-context crates is faster (smaller build
  graph).
- The dep graph now respects layers: `omega-hpc-document` is at the
  bottom, `omega-todo` and `omega-plan` are consumers.
- The `TaskProjection` DTO is the explicit schema for "what the renderer
  needs" and is a stable cross-crate contract.

### Negative

- A sub-workspace adds some indirection. The root `Cargo.toml` now has
  `exclude = ["omega-hpc"]`, and the `test-document-backend` cargo alias
  runs two separate `cargo test` invocations (one per workspace).
- The `omega-context` test has a pre-existing failure unrelated to the
  extraction (`presentation_links` field was added in commit 220bc9e
  without updating a test fixture at `crates/omega-context/src/lib.rs:3308`).
  This blocks `--workspace --all-targets` in the main workspace. The
  extraction did not introduce or fix this; it is left for follow-up.
- `omega-context` keeps adapter coupling to `omega-client` /
  `omega-tools` / `omega-workflow` and stays in the main workspace.

### Neutral

- The `omega-todo` re-export pattern is a transitional convenience for
  downstream call sites. We can drop it later if we want a hard
  re-naming, but it costs nothing and lets us ship the extraction
  without a coordinated downstream rename.
- The `TaskProjection` DTO is intentionally a minimal subset of
  `PlannedTask`. If the renderer needs more fields in the future, we add
  them with `#[serde(default)]` so existing JSONL files still deserialize.
