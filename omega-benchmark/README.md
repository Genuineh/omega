# omega-benchmark

`omega-benchmark` is a standalone workspace member that lives outside `crates/`.

It exists to evaluate Omega as an agent system rather than as a runtime building block.

Current scope:

- tool-calling evaluation inspired by BFCL
- general assistant evaluation inspired by GAIA
- data-generation quality evaluation using judge, win-rate, and human review loops

Canonical planning lives in the repository docs:

- `docs/prds/omega-benchmark-evaluation.md`
- `docs/specs/omega-benchmark-system.md`
- `docs/TODO.md`

This package is intentionally minimal until the benchmark contracts and task breakdown are stabilized.