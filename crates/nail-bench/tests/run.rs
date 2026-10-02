//! The benchmark end to end on a tiny corpus: the generated mail is served
//! by the mock IMAP server, synced by the core, listed, searched and
//! reopened; every Rust metric is measured and the data stays for the
//! Playwright scenarios.

use nail_bench::run::{ARRIVALS, MockMode, RunOptions, run};
use nail_bench::{BenchReport, Budgets, CorpusSpec, MetricName};

fn tiny() -> CorpusSpec {
    CorpusSpec {
        messages: 300,
        folders: 6,
        ..CorpusSpec::small()
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_tiny_run_measures_every_rust_metric() {
    let work = tempfile::tempdir().unwrap();
    let mut options = RunOptions::new(tiny(), work.path());
    options.mock = MockMode::InProcess;
    options.list_samples = 12;
    options.search_rounds = 2;
    let (metrics, facts) = run(&options).await.unwrap();

    for name in MetricName::ALL.into_iter().filter(|m| m.measured_in_rust()) {
        let metric = metrics
            .iter()
            .find(|m| m.metric == name)
            .unwrap_or_else(|| panic!("{name:?} not measured"));
        assert!(
            metric.value.is_finite() && metric.value >= 0.0,
            "{metric:?}"
        );
        assert!(metric.samples >= 1);
    }
    assert!(!metrics.iter().any(|m| !m.metric.measured_in_rust()));

    assert_eq!(facts.messages, 300);
    assert_eq!(facts.folders, 6);
    assert!(facts.corpus_bytes > 300 * 500);
    assert!(facts.data_dir_bytes > 0);
    assert!(facts.sync_messages_per_sec > 0.0);
    let largest = facts.largest_folder.as_ref().unwrap();
    assert_eq!(largest.path, "INBOX");
    assert!(
        largest.total as u64 > ARRIVALS,
        "the arrivals landed in the Inbox"
    );
    // The rare word is found exactly once, the sender queries find mail.
    let rare = facts
        .queries
        .iter()
        .find(|q| q.query == nail_bench::corpus::RARE_WORD)
        .unwrap();
    assert_eq!(rare.hits, 1);
    assert!(facts.queries[0].hits > 0, "{:?}", facts.queries[0]);
    assert!(facts.data_dir.join("nail.sqlite3").exists());

    // The report round-trips and checks against the real budgets.
    let budgets = Budgets::parse(include_str!("../../../e2e/perf/budgets.json")).unwrap();
    let report = BenchReport {
        corpus: tiny(),
        results: budgets.check_with(&metrics, budgets.ci_tolerance),
        metrics,
        facts: Some(facts),
        notes: Vec::new(),
    };
    let json = serde_json::to_string(&report).unwrap();
    let back: BenchReport = serde_json::from_str(&json).unwrap();
    // (Floats may differ in the last bit after a JSON round trip.)
    assert_eq!(back.corpus, report.corpus);
    assert_eq!(back.metrics.len(), report.metrics.len());
    assert_eq!(back.results.len(), report.results.len());
    assert_eq!(back.facts.as_ref().map(|f| f.messages), Some(300));
    // A tiny corpus is far inside every budget.
    assert!(report.passed(), "{}", report.table());

    // Running again in the same work dir starts from scratch.
    let (_, again) = run(&options).await.unwrap();
    assert_eq!(again.messages, 300);
}

#[tokio::test]
async fn invalid_specs_are_refused() {
    let work = tempfile::tempdir().unwrap();
    let options = RunOptions::new(
        CorpusSpec {
            messages: 0,
            ..tiny()
        },
        work.path(),
    );
    assert!(matches!(
        run(&options).await,
        Err(nail_bench::BenchError::Spec(_))
    ));
}
