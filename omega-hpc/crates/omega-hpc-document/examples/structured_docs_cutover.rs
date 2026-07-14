use std::env;
use std::path::PathBuf;

use anyhow::{bail, Context, Result};
use omega_hpc_document::{DocumentMutationMode, OmegaDocument, StructuredDocsValidationReport};

fn main() -> Result<()> {
    let mut args = env::args().skip(1);
    let mut root = env::current_dir().context("failed to resolve current directory")?;
    let mut sources = Vec::new();

    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--root" => {
                let value = args
                    .next()
                    .ok_or_else(|| anyhow::anyhow!("missing value after '--root'"))?;
                root = PathBuf::from(value);
            }
            other => sources.push(other.to_string()),
        }
    }

    if sources.is_empty() {
        sources.push("docs".to_string());
    }

    let documents = OmegaDocument::new(root.clone());

    let result =
        documents.run_structured_docs_cutover(DocumentMutationMode::Apply, sources, None)?;
    let extract = result.extract;
    print_step("extract", &extract.message, extract.warnings.len());
    if !extract.ok {
        bail!("structured docs extraction failed");
    }

    let render = result
        .render
        .ok_or_else(|| anyhow::anyhow!("structured docs render step missing"))?;
    print_step("render", &render.message, render.warnings.len());
    if !render.ok {
        bail!("structured docs render failed");
    }

    let validate = result
        .validate
        .ok_or_else(|| anyhow::anyhow!("structured docs validate step missing"))?;
    print_step("validate", &validate.message, validate.warnings.len());
    if !validate.ok {
        print_validation_failure(validate.validation.as_ref());
        bail!("structured docs validation failed after cutover");
    }

    println!("cutover complete for {}", root.display());
    Ok(())
}

fn print_step(step: &str, message: &str, warnings: usize) {
    println!("[{step}] {message}");
    if warnings > 0 {
        println!("[{step}] warnings: {warnings}");
    }
}

fn print_validation_failure(report: Option<&StructuredDocsValidationReport>) {
    let Some(report) = report else {
        return;
    };
    eprintln!(
        "validation failed: missing={} mismatched={} broken_relations={}",
        report.missing_files.len(),
        report.mismatched_files.len(),
        report.broken_relations.len(),
    );
    for path in &report.missing_files {
        eprintln!("missing: {path}");
    }
    for issue in &report.mismatched_files {
        eprintln!("mismatch: {} :: {}", issue.path, issue.message);
    }
    for issue in &report.broken_relations {
        eprintln!("broken relation: {} :: {}", issue.path, issue.message);
    }
}
