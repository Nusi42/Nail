//! `nail-bench`: runs the benchmark and checks the budgets (WP123). Stub:
//! prints the corpus spec and the budgets it would check.

use clap::Parser;
use nail_bench::{Budgets, CorpusSpec};

/// The performance benchmark.
#[derive(Debug, Parser)]
struct Args {
    /// `standard` (100k messages) or `small`.
    #[arg(long, default_value = "standard")]
    corpus: String,
    /// The budgets file.
    #[arg(long, default_value = "e2e/perf/budgets.json")]
    budgets: std::path::PathBuf,
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let args = Args::parse();
    let spec = match args.corpus.as_str() {
        "small" => CorpusSpec::small(),
        _ => CorpusSpec::standard(),
    };
    let budgets = Budgets::parse(&std::fs::read_to_string(&args.budgets)?)?;
    println!("{}", serde_json::to_string_pretty(&(spec, budgets))?);
    Ok(())
}
