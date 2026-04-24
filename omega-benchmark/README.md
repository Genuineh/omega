# omega-benchmark

`omega-benchmark` is a standalone workspace member that lives outside `crates/`.

It exists to evaluate Omega as an agent system rather than as a runtime building block.

## Tracks

- **Tool Calling** (BFCL-style): tool selection accuracy, argument matching, parallel calls, irrelevance rejection.
- **General Assistant** (GAIA-style): exact match, quasi-exact match, evidence-aware task completion.
- **Data Quality**: schema validity, LLM judge scoring, win rate, human audit pass rate.

## Usage

```sh
# List registered suites
cargo run -p omega-benchmark -- list --suites-dir omega-benchmark/suites

# Run all suites
cargo run -p omega-benchmark -- run --suites-dir omega-benchmark/suites

# Run a specific suite or track
cargo run -p omega-benchmark -- run --suites-dir omega-benchmark/suites --suite tool-basic
cargo run -p omega-benchmark -- run --suites-dir omega-benchmark/suites --track tool-calling

# Compare a run against a baseline
cargo run -p omega-benchmark -- compare <run-id>

# Promote a run to baseline
cargo run -p omega-benchmark -- save-baseline <run-id>
```

## Layout

```
omega-benchmark/
├── src/            # Benchmark library and CLI
├── suites/         # Committed suite manifests and fixtures
│   ├── tool-calling/manifest.json
│   ├── assistant/manifest.json
│   └── data-quality/manifest.json
├── baselines/      # Committed baseline summaries for regression
└── README.md
```

Run artifacts are written to `.omega-state/benchmark/runs/`.

## Related Docs

- `docs/prds/omega-benchmark-evaluation.md`
- `docs/specs/omega-benchmark-system.md`
- `docs/TODO.md`