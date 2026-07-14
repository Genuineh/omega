# omega-hpc

Persistent-context sub-workspace extracted from the main omega monorepo.
Provides the memory + document + doc-cli + project-layout stack as a sibling
Rust workspace that can be built and tested independently of the larger AI /
TUI / workflow crates.

## Members

| Crate                | Source name (pre-extraction) | Purpose                                         |
| -------------------- | ---------------------------- | ----------------------------------------------- |
| `omega-hpc-paths`    | `omega-project-layout`       | Path constants, directory layout, storeignore.  |
| `omega-hpc-memory`   | `omega-memory`               | File store, keyword/vector index, hybrid search, todo store, doc record projections. |
| `omega-hpc-document` | `omega-document`             | Structured doc system, projection rendering, `TaskProjection` DTO, `TodoItem` data types. |
| `omega-hpc-doc-cli`  | `omega-doc-cli`              | `omega-doc` binary.                             |

## Build & test

```sh
cargo check --workspace --all-targets
cargo test  --workspace
cargo fmt --all
```

## Layout

```
omega-hpc/
├── Cargo.toml
├── README.md
├── LICENSE
└── crates/
    ├── omega-hpc-paths/
    ├── omega-hpc-memory/
    ├── omega-hpc-document/
    └── omega-hpc-doc-cli/
```

See `docs/specs/omega-hpc-extraction.md` in the parent repository for the full
extraction contract and `docs/decisions/007-omega-hpc-extraction.md` for the
architecture decision record.
