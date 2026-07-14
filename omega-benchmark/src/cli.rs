use std::path::{Path, PathBuf};

use crate::config::RunConfig;
use crate::registry::SuiteRegistry;
use crate::report::ReportStore;
use crate::result::RunSummary;
use crate::runner::CaseRunner;
use crate::target::{OmegaTarget, StubTarget};

/// CLI output container.
pub struct CliOutput {
    pub stdout: String,
    pub stderr: String,
    pub exit_code: i32,
}

/// Parse and execute the CLI command.
pub fn run(args: impl Iterator<Item = String>) -> CliOutput {
    let args: Vec<String> = args.collect();

    if args.is_empty() {
        return usage();
    }

    match args[0].as_str() {
        "list" => cmd_list(&args[1..]),
        "run" => cmd_run(&args[1..]),
        "compare" => cmd_compare(&args[1..]),
        "save-baseline" => cmd_save_baseline(&args[1..]),
        "help" | "--help" | "-h" => usage(),
        other => CliOutput {
            stdout: String::new(),
            stderr: format!("unknown command: {other}\n\nRun `omega-bench help` for usage.\n"),
            exit_code: 1,
        },
    }
}

fn usage() -> CliOutput {
    CliOutput {
        stdout: "\
omega-bench — Omega benchmark runner

Usage:
  omega-bench list [--suites-dir <path>]
  omega-bench run [--suites-dir <path>] [--config <path>] [--suite <id>] [--track <track>]
                  [--live] [--model <id>]
  omega-bench compare <run-id> [--baseline <id>] [--baselines-dir <path>]
  omega-bench save-baseline <run-id> [--baselines-dir <path>]
  omega-bench help

Commands:
  list            List registered benchmark suites and their cases.
  run             Execute benchmark suites and produce results.
  compare         Compare a run summary against a baseline.
  save-baseline   Promote a run summary to a committed baseline.
  help            Show this message.

Run flags:
  --live          Use the live Omega runtime (requires API key in env).
  --model <id>    Override the model identifier for this run.
"
        .into(),
        stderr: String::new(),
        exit_code: 0,
    }
}

fn resolve_suites_dir(args: &[String]) -> PathBuf {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--suites-dir" {
            if let Some(path) = args.get(i + 1) {
                return PathBuf::from(path);
            }
        }
    }
    // Default: look for suites/ relative to the benchmark crate
    PathBuf::from("omega-benchmark/suites")
}

fn resolve_baselines_dir(args: &[String]) -> PathBuf {
    for (i, arg) in args.iter().enumerate() {
        if arg == "--baselines-dir" {
            if let Some(path) = args.get(i + 1) {
                return PathBuf::from(path);
            }
        }
    }
    PathBuf::from("omega-benchmark/baselines")
}

fn resolve_state_dir() -> PathBuf {
    PathBuf::from(".omega-state")
}

fn cmd_list(args: &[String]) -> CliOutput {
    let suites_dir = resolve_suites_dir(args);
    let registry = match SuiteRegistry::discover(&suites_dir) {
        Ok(r) => r,
        Err(e) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("failed to discover suites: {e}\n"),
                exit_code: 1,
            };
        }
    };

    if registry.is_empty() {
        return CliOutput {
            stdout: format!("No suites found in {}\n", suites_dir.display()),
            stderr: String::new(),
            exit_code: 0,
        };
    }

    let mut out = String::new();
    out.push_str(&format!("Registered suites ({}):\n\n", registry.len()));

    for id in registry.suite_ids() {
        if let Some(suite) = registry.get(&id) {
            out.push_str(&format!(
                "  {} [{}] — {} cases\n    {}\n\n",
                id,
                suite.manifest.track,
                suite.manifest.case_count(),
                suite.manifest.description
            ));
        }
    }

    CliOutput {
        stdout: out,
        stderr: String::new(),
        exit_code: 0,
    }
}

fn cmd_run(args: &[String]) -> CliOutput {
    let suites_dir = resolve_suites_dir(args);
    let state_dir = resolve_state_dir();

    let mut use_live = false;
    let mut model_override: Option<String> = None;

    // Load config if provided, else use defaults
    let mut config = RunConfig::default();
    for (i, arg) in args.iter().enumerate() {
        match arg.as_str() {
            "--config" => {
                if let Some(path) = args.get(i + 1) {
                    match RunConfig::load(Path::new(path)) {
                        Ok(c) => config = c,
                        Err(e) => {
                            return CliOutput {
                                stdout: String::new(),
                                stderr: format!("failed to load config: {e}\n"),
                                exit_code: 1,
                            };
                        }
                    }
                }
            }
            "--suite" => {
                if let Some(id) = args.get(i + 1) {
                    config.suite_filter.push(id.clone());
                }
            }
            "--track" => {
                if let Some(track) = args.get(i + 1) {
                    config.track_filter.push(track.clone());
                }
            }
            "--live" => {
                use_live = true;
            }
            "--model" => {
                if let Some(id) = args.get(i + 1) {
                    model_override = Some(id.clone());
                }
            }
            _ => {}
        }
    }

    if let Some(model) = &model_override {
        config.model = model.clone();
    }

    let registry = match SuiteRegistry::discover(&suites_dir) {
        Ok(r) => r,
        Err(e) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("failed to discover suites: {e}\n"),
                exit_code: 1,
            };
        }
    };

    if registry.is_empty() {
        return CliOutput {
            stdout: format!("No suites found in {}\n", suites_dir.display()),
            stderr: String::new(),
            exit_code: 0,
        };
    }

    // Select target based on --live flag
    let live_target;
    let target: &dyn crate::target::BenchmarkTarget = if use_live {
        let cwd = match std::env::current_dir() {
            Ok(cwd) => cwd,
            Err(e) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: format!("failed to resolve working directory: {e}\n"),
                    exit_code: 1,
                };
            }
        };
        match OmegaTarget::from_env(cwd) {
            Ok(t) => {
                live_target = t;
                &live_target
            }
            Err(e) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: format!("--live requires API credentials: {e}\n"),
                    exit_code: 1,
                };
            }
        }
    } else {
        &StubTarget
    };
    let runner = CaseRunner::new(target, &config);

    let results = runner.run_all(&registry);
    let run_id = format!("run-{}", chrono::Utc::now().format("%Y%m%d-%H%M%S"));
    let suite_track_map = registry.suite_track_map();
    let summary = RunSummary::from_results(
        run_id.clone(),
        config.model.clone(),
        &results,
        &suite_track_map,
    );

    let report = ReportStore::from_root(Path::new("omega-benchmark"), &state_dir);
    let _ = report.save_results(&run_id, &results);
    let _ = report.save_summary(&summary);

    let mut out = String::new();
    out.push_str(&format!("Run: {}\n", summary.run_id));
    out.push_str(&format!("Model: {}\n", summary.model));
    out.push_str(&format!("Timestamp: {}\n\n", summary.timestamp));
    out.push_str(&format!(
        "Results: {} total, {} passed, {} failed, {} errors, {} timeouts, {} skipped\n",
        summary.total_cases,
        summary.passed,
        summary.failed,
        summary.errors,
        summary.timeouts,
        summary.skipped,
    ));
    out.push_str(&format!(
        "Aggregate score: {:.3}\n",
        summary.aggregate_score
    ));
    out.push_str(&format!(
        "Total latency: {}ms | Total tokens: {}\n\n",
        summary.total_latency_ms, summary.total_tokens
    ));

    for ss in &summary.suites {
        out.push_str(&format!(
            "  Suite: {} [{}]\n    Cases: {} | Passed: {} | Failed: {} | Score: {:.3}\n",
            ss.suite_id, ss.track, ss.case_count, ss.passed, ss.failed, ss.aggregate_score
        ));
        for (k, v) in &ss.metrics {
            out.push_str(&format!("    {k}: {v:.3}\n"));
        }
        out.push('\n');
    }

    // Try baseline comparison
    if let Ok(Some(baseline)) = report.load_latest_baseline() {
        let diff = report.compare(&summary, &baseline);
        out.push_str("--- Baseline Comparison ---\n");
        out.push_str(&ReportStore::format_diff(&diff));
    }

    CliOutput {
        stdout: out,
        stderr: String::new(),
        exit_code: 0,
    }
}

fn cmd_compare(args: &[String]) -> CliOutput {
    if args.is_empty() {
        return CliOutput {
            stdout: String::new(),
            stderr: "usage: omega-bench compare <run-id> [--baseline <id>]\n".into(),
            exit_code: 1,
        };
    }

    let run_id = &args[0];
    let state_dir = resolve_state_dir();
    let baselines_dir = resolve_baselines_dir(args);
    let report = ReportStore::new(state_dir.join("benchmark").join("runs"), baselines_dir);

    // Load the run summary
    let run_path = state_dir
        .join("benchmark")
        .join("runs")
        .join(format!("{run_id}-summary.json"));
    let current: RunSummary = match std::fs::read_to_string(&run_path)
        .map_err(anyhow::Error::from)
        .and_then(|s| serde_json::from_str(&s).map_err(anyhow::Error::from))
    {
        Ok(s) => s,
        Err(e) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("failed to load run summary {run_id}: {e}\n"),
                exit_code: 1,
            };
        }
    };

    // Find baseline
    let baseline_id = args
        .iter()
        .enumerate()
        .find(|(_, a)| a.as_str() == "--baseline")
        .and_then(|(i, _)| args.get(i + 1));

    let baseline = if let Some(bid) = baseline_id {
        match report.load_baseline(bid) {
            Ok(Some(b)) => b,
            Ok(None) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: format!("baseline {bid} not found\n"),
                    exit_code: 1,
                };
            }
            Err(e) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: format!("failed to load baseline {bid}: {e}\n"),
                    exit_code: 1,
                };
            }
        }
    } else {
        match report.load_latest_baseline() {
            Ok(Some(b)) => b,
            Ok(None) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: "no baseline found for comparison\n".into(),
                    exit_code: 1,
                };
            }
            Err(e) => {
                return CliOutput {
                    stdout: String::new(),
                    stderr: format!("failed to load latest baseline: {e}\n"),
                    exit_code: 1,
                };
            }
        }
    };

    let diff = report.compare(&current, &baseline);
    let text = ReportStore::format_diff(&diff);

    CliOutput {
        stdout: text,
        stderr: String::new(),
        exit_code: 0,
    }
}

fn cmd_save_baseline(args: &[String]) -> CliOutput {
    if args.is_empty() {
        return CliOutput {
            stdout: String::new(),
            stderr: "usage: omega-bench save-baseline <run-id>\n".into(),
            exit_code: 1,
        };
    }

    let run_id = &args[0];
    let state_dir = resolve_state_dir();
    let baselines_dir = resolve_baselines_dir(args);
    let report = ReportStore::new(state_dir.join("benchmark").join("runs"), baselines_dir);

    let run_path = state_dir
        .join("benchmark")
        .join("runs")
        .join(format!("{run_id}-summary.json"));
    let summary: RunSummary = match std::fs::read_to_string(&run_path)
        .map_err(anyhow::Error::from)
        .and_then(|s| serde_json::from_str(&s).map_err(anyhow::Error::from))
    {
        Ok(s) => s,
        Err(e) => {
            return CliOutput {
                stdout: String::new(),
                stderr: format!("failed to load run summary {run_id}: {e}\n"),
                exit_code: 1,
            };
        }
    };

    match report.save_baseline(&summary) {
        Ok(path) => CliOutput {
            stdout: format!("Baseline saved: {}\n", path.display()),
            stderr: String::new(),
            exit_code: 0,
        },
        Err(e) => CliOutput {
            stdout: String::new(),
            stderr: format!("failed to save baseline: {e}\n"),
            exit_code: 1,
        },
    }
}
