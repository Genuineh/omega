# Changelog

All notable changes to omega are documented in this file. Dates are in
`YYYY-MM-DD` and reflect when the change landed on the main branch.

The format is loosely based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and the project follows semantic versioning for public Rust API surfaces.

## [Unreleased]

### Added

- `omega-hpc/` sub-workspace: a self-contained Cargo workspace that owns
  `omega-hpc-paths`, `omega-hpc-memory`, `omega-hpc-document`, and
  `omega-hpc-doc-cli`. Has its own `[workspace.package]`,
  `[workspace.dependencies]`, release profile
  (`opt-level = "s"`, `lto = true`, `codegen-units = 1`,
  `panic = "abort"`, `strip = true`), `README.md`, `LICENSE`, and
  `.gitignore`. The main workspace excludes it via
  `Cargo.toml`'s `exclude = ["omega-hpc"]`.
- `docs/specs/omega-hpc-extraction.md` — umbrella spec for the
  extraction (goals, non-goals, dependency direction, sub-workspace
  layout, API stability, verification matrix, follow-up).
- `docs/decisions/007-omega-hpc-extraction.md` — ADR-007 recording
  context, decision, alternatives, and consequences.
- `docs-data/manifest.json` and `docs-data/render/render-state.json`
  now carry `content_revision = 123`,
  `projection_version = 51`,
  `last_generation_id = "gen_000051_r000123"`.

### Changed

- `Cargo.toml` (root) — removed `crates/omega-{doc-cli,document,memory,project-layout}`
  from `members`; added `exclude = ["omega-hpc"]`.
- `.cargo/config.toml` — `test-document-backend` alias now points at
  the hpc workspace manifest; added
  `test-document-backend-main` alias for the main
  `omega-context` document-backend integration test;
  `test-document-commands` unchanged.
- Eleven downstream crates (`omega-app`, `omega-context`, `omega-hooks`,
  `omega-keymap`, `omega-plan`, `omega-project`, `omega-session`,
  `omega-theme`, `omega-todo`, `omega-tui`, `omega-workflow`) switched
  from `path = "../omega-{memory,document,doc-cli,project-layout}"`
  to `path = "../../omega-hpc/crates/omega-hpc-*"`. Source-level
  `use omega_{memory,document,project_layout,doc_cli}` renames to
  `use omega_hpc_{memory,document,paths,doc_cli}`.
- `omega-context/Cargo.toml` — `document-backend` feature now depends
  on `dep:omega-hpc-document`; `omega-hpc-document` is declared
  `optional = true`; the `omega-hpc-memory` path dep is added.
- `omega-todo` is now a thin re-export layer for the todo data types
  (`TodoItem`, `TodoStatus`, `TodoManager`, `SharedTodoManager`,
  `MAX_TODO_ITEMS`, `TODO_REMINDER`) and continues to own the tool
  handlers (`TodoToolHandler`, `TodoWriteHandler`, `TodoReadHandler`).
- `AGENTS.md` — Project Structure section now describes the
  `omega-hpc/` sub-workspace, the cross-workspace dependency
  direction, and the API stability contract.
- 14 affected specs and `docs/guide/omega-dev-guide.md` received an
  "Implementation Note" footer pointing to the extraction spec and
  ADR-007.
- `docs/README.md` and `docs/decisions/README.md` got a new
  `omega-hpc` row linking to the spec and ADR.

### Removed

- `crates/omega-project-layout/` — moved to
  `omega-hpc/crates/omega-hpc-paths/`; package renamed to
  `omega-hpc-paths`; all public const names
  (`STORE_*_PATH`, `MEMORY_*_PATH`, `DOCS_DATA_*_PATH`, …) and the
  `OmegaProjectLayout` type unchanged.
- `crates/omega-memory/` — moved to
  `omega-hpc/crates/omega-hpc-memory/`; package renamed to
  `omega-hpc-memory`; public types (`StepSummary`,
  `LocalMemoryStore`, `ProjectObservation`, `ObservationQuery`,
  `MemoryQueryHit`, …) unchanged.
- `crates/omega-document/` — moved to
  `omega-hpc/crates/omega-hpc-document/` (including
  `examples/structured_docs_cutover.rs`); package renamed to
  `omega-hpc-document`; public types (`OmegaDocument`,
  `DocumentOp`, `StructuredDocumentRecord`, `StructuredDocsManifest`,
  `FileRecord`, `SearchQuery`, `SearchResult`, …) unchanged.
- `crates/omega-doc-cli/` — moved to
  `omega-hpc/crates/omega-hpc-doc-cli/`; package renamed to
  `omega-hpc-doc-cli`; `[[bin]] name = "omega-doc"` retained;
  subcommands, exit codes, and JSON / human output formats
  unchanged.
- Backward dependency edges: `omega-document` no longer depends on
  `omega-todo` or `omega-plan`. The `TaskProjection` DTO
  (`omega-hpc-document::structured_docs::TaskProjection`) is read
  directly from `docs-data/tasks/project-tasks.jsonl`; the
  `PlannedTask → TaskProjection` adapter lives in `omega-plan`.

### Migration notes

- `cargo build` and `cargo test` now work in two workspaces:
  main `omega/` and `omega-hpc/`. `cargo test-document-backend`
  exercises the hpc workspace; `cargo test-document-backend-main`
  exercises the main workspace `omega-context/document-backend`
  feature. `cargo test-document-commands` is unchanged.
- A pre-existing test failure in
  `crates/omega-context/src/lib.rs:3308` (missing
  `presentation_links` field in a `SelectedProjectTaskContext`
  initializer, present on `main` since the task-store unification
  commit `220bc9e`) blocks `cargo test -p omega-context
  --features omega-context/document-backend` and any
  `cargo test --all-targets` in the main workspace. Recorded as
  out-of-scope negative consequence in ADR-007; lib-only
  `cargo check --workspace` in the main workspace passes.
- `docs/TODO.md` was emergency-projection-repaired to drop the
  completed `Task 38 / 38A ~ 38I` entries from Active Tasks,
  remove the high-priority pointer, and add a new
  `omega-hpc extraction baseline is complete` entry under
  Current Baseline. The completion is durably captured in
  `docs/specs/omega-hpc-extraction.md` and
  `docs/decisions/007-omega-hpc-extraction.md`.
