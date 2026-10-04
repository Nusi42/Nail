//! `nail-bench`: runs the benchmark and checks the budgets (plan M4c
//! WP123; developer tool, its output is not user-facing).
//!
//! ```text
//! nail-bench [run] [--corpus standard|small] [--work-dir DIR] [--budgets FILE]
//!            [--report FILE] [--tolerant] [--in-process] [--no-check]
//! nail-bench check --report FILE [--report FILE …] [--budgets FILE] [--tolerant]
//! nail-bench corpus [--corpus …]          # counts of the generated corpus
//! nail-bench mock-imap --spec JSON         # internal: the mock server child
//! ```
//!
//! `run` leaves the synced data in `<work-dir>/data` (the Playwright perf
//! scenarios open it with `nail serve`) and writes the report to
//! `<work-dir>/report-rust.json` unless `--report` says otherwise. The work
//! directory defaults to `<checkout>/.nail-build/perf/<corpus>` (gitignored),
//! never tracked files. The exit code is 1 when a budget is exceeded.

use std::path::{Path, PathBuf};
use std::process::ExitCode;

use clap::{Args, Parser, Subcommand};
use nail_bench::corpus::Corpus;
use nail_bench::num::f64_of;
use nail_bench::run::{MockMode, RunOptions};
use nail_bench::{BenchError, BenchReport, Budgets, CorpusSpec, Metric, merge_metrics};

/// The performance benchmark.
#[derive(Debug, Parser)]
#[command(name = "nail-bench", version)]
struct Cli {
    #[command(subcommand)]
    command: Option<Command>,
    #[command(flatten)]
    run: RunArgs,
}

#[derive(Debug, Subcommand)]
enum Command {
    /// Generate the corpus, sync it, measure (the default).
    Run(RunArgs),
    /// Check one or more reports against the budgets.
    Check(CheckArgs),
    /// Print the counts of a generated corpus.
    Corpus(CorpusArgs),
    /// Serve a corpus from the mock IMAP server (internal; started by
    /// `run`).
    MockImap {
        /// The corpus spec as JSON.
        #[arg(long)]
        spec: String,
    },
}

#[derive(Debug, Clone, Args)]
struct RunArgs {
    /// `standard` (100k messages) or `small` (2,000).
    #[arg(long, default_value = "standard")]
    corpus: String,
    /// Override the number of messages of the corpus.
    #[arg(long)]
    messages: Option<u64>,
    /// Where the data, config and report go (default: the build directory).
    #[arg(long)]
    work_dir: Option<PathBuf>,
    /// The budgets file.
    #[arg(long, default_value = "e2e/perf/budgets.json")]
    budgets: PathBuf,
    /// Where the report goes (default: `<work-dir>/report-rust.json`).
    #[arg(long)]
    report: Option<PathBuf>,
    /// Apply the budgets' CI tolerance.
    #[arg(long)]
    tolerant: bool,
    /// Run the mock server in this process (its memory then counts).
    #[arg(long)]
    in_process: bool,
    /// Measure only; do not fail on budgets.
    #[arg(long)]
    no_check: bool,
}

#[derive(Debug, Clone, Args)]
struct CheckArgs {
    /// Reports to merge (later ones replace earlier measurements).
    #[arg(long = "report", required = true)]
    reports: Vec<PathBuf>,
    /// The budgets file.
    #[arg(long, default_value = "e2e/perf/budgets.json")]
    budgets: PathBuf,
    /// Apply the budgets' CI tolerance.
    #[arg(long)]
    tolerant: bool,
    /// Write the merged report here.
    #[arg(long)]
    out: Option<PathBuf>,
}

#[derive(Debug, Clone, Args)]
struct CorpusArgs {
    /// `standard` or `small`.
    #[arg(long, default_value = "standard")]
    corpus: String,
    /// Override the number of messages.
    #[arg(long)]
    messages: Option<u64>,
}

fn spec_of(name: &str, messages: Option<u64>) -> Result<CorpusSpec, BenchError> {
    let mut spec = CorpusSpec::named(name)
        .ok_or_else(|| BenchError::Spec(format!("unknown corpus {name:?}")))?;
    if let Some(m) = messages {
        spec.messages = m;
    }
    Ok(spec)
}

/// `NAIL_BUILD_DIR`, else the checkout's gitignored `.nail-build` (each work
/// folder has its own).
fn build_dir() -> PathBuf {
    std::env::var_os("NAIL_BUILD_DIR").map_or_else(
        || Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.nail-build"),
        PathBuf::from,
    )
}

fn read_budgets(path: &Path) -> Result<Budgets, BenchError> {
    let budgets = Budgets::parse(&std::fs::read_to_string(path)?)?;
    let issues = budgets.issues();
    if !issues.is_empty() {
        return Err(BenchError::Spec(format!(
            "{}: {}",
            path.display(),
            issues.join("; ")
        )));
    }
    Ok(budgets)
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) -> Result<(), BenchError> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::write(path, serde_json::to_vec_pretty(value)?)?;
    Ok(())
}

async fn run(args: RunArgs) -> Result<bool, BenchError> {
    let spec = spec_of(&args.corpus, args.messages)?;
    let budgets = read_budgets(&args.budgets)?;
    let work_dir = args
        .work_dir
        .clone()
        .unwrap_or_else(|| build_dir().join("perf").join(&args.corpus));
    let mut options = RunOptions::new(spec.clone(), &work_dir);
    if !args.in_process {
        options.mock = MockMode::Child(std::env::current_exe()?);
    }
    let (metrics, facts) = nail_bench::run::run(&options).await?;
    let factor = if args.tolerant {
        budgets.ci_tolerance
    } else {
        1.0
    };
    let mut notes = Vec::new();
    if CorpusSpec::named(&budgets.corpus).as_ref() != Some(&spec) {
        notes.push(format!(
            "the budgets are for the {} corpus; this run used another one (smoke test only)",
            budgets.corpus
        ));
    }
    if args.tolerant {
        notes.push(format!("CI tolerance x{factor} applied"));
    }
    if args.in_process {
        notes.push("mock server in this process: its memory counts".into());
    }
    let report = BenchReport {
        corpus: spec,
        results: budgets.check_with(&metrics, factor),
        metrics,
        facts: Some(facts),
        notes,
    };
    let path = args
        .report
        .unwrap_or_else(|| work_dir.join("report-rust.json"));
    write_json(&path, &report)?;
    print!("{}", report.table());
    if let Some(facts) = &report.facts {
        println!(
            "synced {} messages ({:.0} per second, {:.1} MiB served, data dir {:.1} MiB)",
            facts.messages,
            facts.sync_messages_per_sec,
            f64_of(facts.corpus_bytes) / 1_048_576.0,
            f64_of(facts.data_dir_bytes) / 1_048_576.0
        );
        for q in &facts.queries {
            println!(
                "  search {:<40} {:>6} hits  median {:>7.1} ms  max {:>7.1} ms",
                q.query, q.hits, q.median_ms, q.max_ms
            );
        }
        println!("data: {}", facts.data_dir.display());
    }
    println!("report: {}", path.display());
    Ok(args.no_check || report.passed())
}

fn check(args: &CheckArgs) -> Result<bool, BenchError> {
    let budgets = read_budgets(&args.budgets)?;
    let mut lists = Vec::new();
    let mut corpus = None;
    let mut facts = None;
    for path in &args.reports {
        let text = std::fs::read_to_string(path)?;
        // A full report, or the Playwright scenarios' `{ metrics: [...] }`.
        let value: serde_json::Value = serde_json::from_str(&text)?;
        if let Ok(report) = serde_json::from_value::<BenchReport>(value.clone()) {
            corpus.get_or_insert(report.corpus);
            if report.facts.is_some() {
                facts = report.facts;
            }
            lists.push(report.metrics);
        } else {
            let metrics: Vec<Metric> =
                serde_json::from_value(value.get("metrics").cloned().unwrap_or_default())?;
            lists.push(metrics);
        }
    }
    let metrics = merge_metrics(&lists);
    let factor = if args.tolerant {
        budgets.ci_tolerance
    } else {
        1.0
    };
    let mut notes = Vec::new();
    let missing: Vec<&str> = nail_bench::MetricName::ALL
        .iter()
        .filter(|m| !metrics.iter().any(|x| x.metric == **m))
        .map(|m| m.as_str())
        .collect();
    if !missing.is_empty() {
        notes.push(format!("not measured: {}", missing.join(", ")));
    }
    if args.tolerant {
        notes.push(format!("CI tolerance x{factor} applied"));
    }
    let report = BenchReport {
        corpus: corpus.unwrap_or_else(CorpusSpec::standard),
        results: budgets.check_with(&metrics, factor),
        metrics,
        facts,
        notes,
    };
    if let Some(out) = &args.out {
        write_json(out, &report)?;
    }
    print!("{}", report.table());
    Ok(report.passed())
}

fn corpus(args: &CorpusArgs) -> Result<(), BenchError> {
    let spec = spec_of(&args.corpus, args.messages)?;
    let corpus = Corpus::new(spec);
    let stats = corpus.stats();
    println!(
        "{} messages, {:.1} MiB, {} with attachments, {} replies, {} unread",
        stats.messages,
        f64_of(stats.bytes) / 1_048_576.0,
        stats.with_attachment,
        stats.replies,
        stats.unread
    );
    for (folder, count) in corpus.folders().iter().zip(&stats.per_folder) {
        println!("  {:<28} {count}", folder.name);
    }
    Ok(())
}

fn main() -> ExitCode {
    // Progress on stderr with `NAIL_LOG=info` (stdout carries the results
    // and the mock child's protocol).
    drop(
        tracing_subscriber::fmt()
            .with_env_filter(
                tracing_subscriber::EnvFilter::try_from_env("NAIL_LOG")
                    .unwrap_or_else(|_| tracing_subscriber::EnvFilter::new("warn")),
            )
            .with_writer(std::io::stderr)
            .try_init(),
    );
    let cli = Cli::parse();
    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(r) => r,
        Err(e) => {
            eprintln!("nail-bench: {e}");
            return ExitCode::FAILURE;
        }
    };
    let result = match cli.command {
        None => runtime.block_on(run(cli.run)),
        Some(Command::Run(args)) => runtime.block_on(run(args)),
        Some(Command::Check(args)) => check(&args),
        Some(Command::Corpus(args)) => corpus(&args).map(|()| true),
        Some(Command::MockImap { spec }) => serde_json::from_str::<CorpusSpec>(&spec)
            .map_err(BenchError::from)
            .and_then(|spec| runtime.block_on(nail_bench::mock::serve_child(spec)))
            .map(|()| true),
    };
    match result {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => {
            eprintln!("nail-bench: a budget was exceeded");
            ExitCode::from(1)
        }
        Err(e) => {
            eprintln!("nail-bench: {e}");
            ExitCode::from(2)
        }
    }
}
