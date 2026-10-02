//! The performance benchmark harness (plan M4c WP123; contract ADR 0094).
//!
//! The interfaces both harnesses share: the generated corpus
//! ([`CorpusSpec`]), the metrics ([`MetricName`], [`Metric`]), the budgets
//! file `e2e/perf/budgets.json` ([`Budgets`]) and the report
//! ([`BenchReport`]). WP123 fills in the corpus generator and the runs
//! (initial sync from the mock IMAP server, list paging, search latency,
//! cold start, memory); the Playwright side measures scrolling and the
//! first paint against the same budgets.

use serde::{Deserialize, Serialize};

/// The generated corpus.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct CorpusSpec {
    /// Messages in total.
    pub messages: u64,
    /// Folders they are spread over (the Inbox gets the largest share).
    pub folders: u32,
    /// Share of messages that are replies in a thread (0..=1).
    pub thread_ratio: f64,
    /// Share of messages with an attachment (0..=1).
    pub attachment_ratio: f64,
    /// Share of HTML messages (0..=1).
    pub html_ratio: f64,
    /// Mean body size in bytes.
    pub mean_body_bytes: u32,
    /// Random seed: the same spec always generates the same corpus.
    pub seed: u64,
}

impl CorpusSpec {
    /// The 100k-message benchmark the budgets are for.
    pub fn standard() -> Self {
        Self {
            messages: 100_000,
            folders: 40,
            thread_ratio: 0.4,
            attachment_ratio: 0.1,
            html_ratio: 0.6,
            mean_body_bytes: 4_000,
            seed: 0x4E41_494C,
        }
    }

    /// A quick corpus for CI smoke runs and tests (same shape, 2,000
    /// messages).
    pub fn small() -> Self {
        Self {
            messages: 2_000,
            folders: 8,
            ..Self::standard()
        }
    }

    /// Problems (empty = valid).
    pub fn issues(&self) -> Vec<String> {
        let mut issues = Vec::new();
        if self.messages == 0 {
            issues.push("messages must be > 0".to_owned());
        }
        if self.folders == 0 {
            issues.push("folders must be > 0".to_owned());
        }
        for (name, value) in [
            ("thread-ratio", self.thread_ratio),
            ("attachment-ratio", self.attachment_ratio),
            ("html-ratio", self.html_ratio),
        ] {
            if !(0.0..=1.0).contains(&value) {
                issues.push(format!("{name} must be within 0..=1"));
            }
        }
        issues
    }
}

/// What is measured.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum MetricName {
    /// Initial sync of the whole corpus from the mock IMAP server (s).
    InitialSyncSecs,
    /// Search latency, 95th percentile over the query set (ms).
    SearchP95Ms,
    /// One message-list page (`ListMessages`), 95th percentile (ms).
    ListPageP95Ms,
    /// Core start on the synced data dir until the first list answers (ms).
    ColdStartMs,
    /// Peak working set of the process (MiB).
    PeakMemoryMib,
    /// Frame time while scrolling the list, 95th percentile (ms;
    /// Playwright).
    ScrollFrameP95Ms,
    /// Frames over 50 ms while scrolling (count; Playwright).
    ScrollLongFrames,
}

impl MetricName {
    /// Every metric.
    pub const ALL: [MetricName; 7] = [
        Self::InitialSyncSecs,
        Self::SearchP95Ms,
        Self::ListPageP95Ms,
        Self::ColdStartMs,
        Self::PeakMemoryMib,
        Self::ScrollFrameP95Ms,
        Self::ScrollLongFrames,
    ];

    /// Whether the Rust harness measures it (else Playwright).
    pub fn measured_in_rust(self) -> bool {
        !matches!(self, Self::ScrollFrameP95Ms | Self::ScrollLongFrames)
    }
}

/// One measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    /// What.
    pub metric: MetricName,
    /// The value (in the metric's unit).
    pub value: f64,
    /// How many samples it summarizes.
    #[serde(default)]
    pub samples: u32,
}

/// One budget: the metric must stay at or below `max`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budget {
    /// What.
    pub metric: MetricName,
    /// The largest allowed value.
    pub max: f64,
}

/// The budgets file (`e2e/perf/budgets.json`).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Budgets {
    /// Which corpus the budgets are for (`standard` / `small`).
    pub corpus: String,
    /// The budgets.
    pub budgets: Vec<Budget>,
}

/// A budget checked against a measurement.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BudgetResult {
    /// The budget.
    pub budget: Budget,
    /// The measured value; `None` = not measured in this run.
    pub value: Option<f64>,
    /// Whether it held (unmeasured budgets do not fail).
    pub passed: bool,
}

impl Budgets {
    /// Reads the budgets JSON.
    pub fn parse(text: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(text)
    }

    /// Checks measurements against the budgets.
    pub fn check(&self, metrics: &[Metric]) -> Vec<BudgetResult> {
        self.budgets
            .iter()
            .map(|budget| {
                let value = metrics
                    .iter()
                    .find(|m| m.metric == budget.metric)
                    .map(|m| m.value);
                BudgetResult {
                    budget: budget.clone(),
                    value,
                    passed: value.is_none_or(|v| v <= budget.max),
                }
            })
            .collect()
    }
}

/// The result of one benchmark run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct BenchReport {
    /// The corpus.
    pub corpus: CorpusSpec,
    /// What was measured.
    pub metrics: Vec<Metric>,
    /// The budgets checked.
    pub results: Vec<BudgetResult>,
}

impl BenchReport {
    /// Whether every measured budget held.
    pub fn passed(&self) -> bool {
        self.results.iter().all(|r| r.passed)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;

    const BUDGETS: &str = include_str!("../../../e2e/perf/budgets.json");

    #[test]
    fn the_budgets_file_covers_every_metric() {
        let budgets = Budgets::parse(BUDGETS).unwrap();
        assert_eq!(budgets.corpus, "standard");
        for metric in MetricName::ALL {
            assert!(
                budgets.budgets.iter().any(|b| b.metric == metric),
                "{metric:?} has no budget"
            );
        }
        assert_eq!(budgets.budgets.len(), MetricName::ALL.len());
    }

    #[test]
    fn budgets_check_measurements() {
        let budgets = Budgets::parse(BUDGETS).unwrap();
        let metrics = [
            Metric {
                metric: MetricName::SearchP95Ms,
                value: 10.0,
                samples: 50,
            },
            Metric {
                metric: MetricName::ColdStartMs,
                value: 1e9,
                samples: 1,
            },
        ];
        let results = budgets.check(&metrics);
        let search = results
            .iter()
            .find(|r| r.budget.metric == MetricName::SearchP95Ms)
            .unwrap();
        assert!(search.passed);
        let cold = results
            .iter()
            .find(|r| r.budget.metric == MetricName::ColdStartMs)
            .unwrap();
        assert!(!cold.passed);
        let unmeasured = results
            .iter()
            .find(|r| r.budget.metric == MetricName::PeakMemoryMib)
            .unwrap();
        assert!(unmeasured.passed && unmeasured.value.is_none());
        let report = BenchReport {
            corpus: CorpusSpec::small(),
            metrics: metrics.to_vec(),
            results,
        };
        assert!(!report.passed());
    }

    #[test]
    fn corpus_specs() {
        assert!(CorpusSpec::standard().issues().is_empty());
        assert_eq!(CorpusSpec::standard().messages, 100_000);
        assert!(CorpusSpec::small().issues().is_empty());
        let bad = CorpusSpec {
            messages: 0,
            html_ratio: 2.0,
            ..CorpusSpec::small()
        };
        assert_eq!(bad.issues().len(), 2);
        assert!(MetricName::InitialSyncSecs.measured_in_rust());
        assert!(!MetricName::ScrollLongFrames.measured_in_rust());
    }
}
