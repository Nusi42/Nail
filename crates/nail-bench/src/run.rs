//! The Rust side of the benchmark: initial sync of the corpus from the mock
//! IMAP server, incremental sync of new arrivals, message-list pages,
//! search latency, cold start of the core on the synced data and the peak
//! working set.
//!
//! Every phase talks to the core the way clients do — through
//! [`nail_core::Service`] and [`nail_api::Request`]s — except the sync
//! passes, which run with [`Service::sync_account`] so the time is the
//! pass itself (no poll interval in between).

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, Instant};

use nail_api::engine::{MemorySecretStore, SecretStore};
use nail_api::{
    Account, AccountId, AccountKind, Folder, FolderRole, Request, Response, SearchQuery,
};
use nail_core::{ImapAccount, Service, ServiceOptions};
use nail_proto_imap::{Credentials, ImapConfig, Security};
use serde::{Deserialize, Serialize};

use crate::corpus::Corpus;
use crate::mock::MockMailServer;
use crate::num::{f64_of, f64_of_len, usize_of};
use crate::{BenchError, CorpusSpec, Metric, MetricName};

/// The account the benchmark syncs.
pub const BENCH_ACCOUNT_ID: &str = "bench";

/// New messages delivered for the incremental sync.
pub const ARRIVALS: u64 = 200;

/// Messages per list page (the UI's default page size).
pub const PAGE_SIZE: u32 = 100;

/// Where the mock server runs.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MockMode {
    /// In this process (its memory counts towards the peak).
    InProcess,
    /// In a child process started from this executable
    /// (`<exe> mock-imap`).
    Child(PathBuf),
}

/// How to run the benchmark.
#[derive(Debug, Clone)]
pub struct RunOptions {
    /// The corpus.
    pub spec: CorpusSpec,
    /// A directory for the data and config directories (wiped first).
    pub work_dir: PathBuf,
    /// Where the mock server runs.
    pub mock: MockMode,
    /// Repetitions of each search query (the first one is cold).
    pub search_rounds: u32,
    /// List pages sampled.
    pub list_samples: u32,
}

impl RunOptions {
    /// Defaults for `spec` in `work_dir`, mock in this process.
    pub fn new(spec: CorpusSpec, work_dir: impl Into<PathBuf>) -> Self {
        Self {
            spec,
            work_dir: work_dir.into(),
            mock: MockMode::InProcess,
            search_rounds: 3,
            list_samples: 60,
        }
    }
}

/// What a run found besides the metrics (for the report and the
/// Playwright scenarios that reuse the data).
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct RunFacts {
    /// The data directory with the synced corpus.
    pub data_dir: PathBuf,
    /// The config directory.
    pub config_dir: PathBuf,
    /// The synced account.
    pub account_id: String,
    /// Messages stored after the initial sync.
    pub messages: u64,
    /// Raw bytes the server served.
    pub corpus_bytes: u64,
    /// The largest folder (the Playwright scenarios scroll it).
    pub largest_folder: Option<FolderFact>,
    /// Folders synced.
    pub folders: u32,
    /// Search queries with their hit counts (first round).
    pub queries: Vec<QueryFact>,
    /// Throughput of the initial sync (messages per second).
    pub sync_messages_per_sec: f64,
    /// Data directory size after the sync (bytes).
    pub data_dir_bytes: u64,
}

/// A folder of the synced account.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct FolderFact {
    /// Id.
    pub id: String,
    /// Path.
    pub path: String,
    /// Messages.
    pub total: u32,
}

/// One search query of the run.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct QueryFact {
    /// The query.
    pub query: String,
    /// Total hits.
    pub hits: u32,
    /// Slowest round (ms).
    pub max_ms: f64,
    /// Median round (ms).
    pub median_ms: f64,
}

/// The `p`-th percentile (0..=100, nearest rank) of `samples`.
pub fn percentile(samples: &[f64], p: f64) -> f64 {
    if samples.is_empty() {
        return 0.0;
    }
    let mut sorted = samples.to_vec();
    sorted.sort_by(f64::total_cmp);
    let rank = usize_of(((p / 100.0) * f64_of_len(sorted.len())).ceil());
    sorted[rank.clamp(1, sorted.len()) - 1]
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

/// Options of a benchmark service: no config watcher, secrets in memory
/// (shared by every open of the same data, like a keychain), the seeded
/// runs' offline settings.
fn service_options(
    data_dir: &Path,
    config_dir: &Path,
    secrets: &Arc<dyn SecretStore>,
) -> ServiceOptions {
    let mut options = ServiceOptions::new(data_dir, config_dir);
    options.watch_config = false;
    options.secrets = Some(Arc::clone(secrets));
    nail_core::demo::demo_options(options)
}

/// Stops a service and waits until its files are free (the search index
/// writer holds a lock until the last clone of the service is gone).
async fn close(service: Service) {
    service.shutdown().await;
    drop(service);
    tokio::time::sleep(Duration::from_millis(750)).await;
}

fn unexpected(what: &str, response: &Response) -> BenchError {
    BenchError::Core(format!("{what}: unexpected answer {response:?}"))
}

async fn folders(service: &Service, account: &AccountId) -> Result<Vec<Folder>, BenchError> {
    match service
        .handle(Request::ListFolders {
            account_id: account.clone(),
        })
        .await
    {
        Response::Folders { folders } => Ok(folders),
        other => Err(unexpected("ListFolders", &other)),
    }
}

async fn list_page(
    service: &Service,
    folder: &Folder,
    offset: u32,
) -> Result<(u32, usize), BenchError> {
    match service
        .handle(Request::ListMessages {
            folder_id: folder.id.clone(),
            offset,
            limit: PAGE_SIZE,
        })
        .await
    {
        Response::Messages {
            total, messages, ..
        } => Ok((total, messages.len())),
        other => Err(unexpected("ListMessages", &other)),
    }
}

/// One page of the folder's conversations (`ListThreads`): the total and
/// how many rows came back.
async fn thread_page(
    service: &Service,
    folder: &Folder,
    offset: u32,
) -> Result<(u32, usize), BenchError> {
    match service
        .handle(Request::ListThreads {
            folder_id: folder.id.clone(),
            offset,
            limit: PAGE_SIZE,
        })
        .await
    {
        Response::Threads { total, threads, .. } => Ok((total, threads.len())),
        other => Err(unexpected("ListThreads", &other)),
    }
}

async fn search(service: &Service, text: &str) -> Result<u32, BenchError> {
    match service
        .handle(Request::Search {
            query: SearchQuery {
                text: text.to_owned(),
                account_id: None,
                folder_id: None,
                offset: 0,
                limit: 50,
                sort: nail_api::search::SearchSort::default(),
            },
        })
        .await
    {
        Response::SearchResults { total, .. } => Ok(total),
        other => Err(unexpected(&format!("Search {text:?}"), &other)),
    }
}

fn dir_size(path: &Path) -> u64 {
    let Ok(entries) = std::fs::read_dir(path) else {
        return 0;
    };
    entries
        .filter_map(Result::ok)
        .map(|e| match e.file_type() {
            Ok(t) if t.is_dir() => dir_size(&e.path()),
            Ok(_) => e.metadata().map_or(0, |m| m.len()),
            Err(_) => 0,
        })
        .sum()
}

/// Deterministic list offsets: the first pages (what people look at), the
/// middle and the end of the largest folders.
fn list_offsets(total: u32, samples: u32, seed: u64) -> Vec<u32> {
    let pages = total.div_ceil(PAGE_SIZE).max(1);
    let mut x = seed | 1;
    (0..samples)
        .map(|i| {
            let page = match i % 4 {
                0 => 0,
                1 => 1.min(pages - 1),
                _ => {
                    // xorshift: spread over the folder.
                    x ^= x << 13;
                    x ^= x >> 7;
                    x ^= x << 17;
                    u32::try_from(x % u64::from(pages)).unwrap_or(0)
                }
            };
            page * PAGE_SIZE
        })
        .collect()
}

/// Runs the benchmark: answers the metrics and what it found. The data
/// directory stays in `options.work_dir` (see [`RunFacts::data_dir`]).
pub async fn run(options: &RunOptions) -> Result<(Vec<Metric>, RunFacts), BenchError> {
    let issues = options.spec.issues();
    if !issues.is_empty() {
        return Err(BenchError::Spec(issues.join("; ")));
    }
    let data_dir = options.work_dir.join("data");
    let config_dir = options.work_dir.join("config");
    for dir in [&data_dir, &config_dir] {
        if dir.exists() {
            std::fs::remove_dir_all(dir)?;
        }
        std::fs::create_dir_all(dir)?;
    }
    tracing::info!(
        messages = options.spec.messages,
        "starting the mock IMAP server"
    );
    let mut mock = match &options.mock {
        MockMode::InProcess => MockMailServer::start_in_process(&options.spec).await?,
        MockMode::Child(exe) => MockMailServer::start_child(exe, &options.spec)?,
    };
    let ready = mock.ready().clone();
    let corpus = Corpus::new(options.spec.clone());
    let account = AccountId::new(BENCH_ACCOUNT_ID);
    let mut metrics = Vec::new();
    let mut facts = RunFacts {
        data_dir: data_dir.clone(),
        config_dir: config_dir.clone(),
        account_id: account.to_string(),
        corpus_bytes: ready.bytes,
        ..RunFacts::default()
    };

    // --- Initial sync -------------------------------------------------
    let secrets: Arc<dyn SecretStore> = Arc::new(MemorySecretStore::new());
    let service = Service::open(service_options(&data_dir, &config_dir, &secrets))?;
    service
        .add_imap_account(
            Account {
                id: account.clone(),
                name: "Benchmark".into(),
                email: ready.username.clone(),
                kind: AccountKind::Imap,
            },
            imap_settings(&ready),
        )
        .await?;
    tracing::info!("initial sync");
    let started = Instant::now();
    let report = service.sync_account(&account).await?;
    let initial = started.elapsed();
    if !report.failed_folders.is_empty() {
        return Err(BenchError::Core(format!(
            "folders failed to sync: {:?}",
            report.failed_folders
        )));
    }
    if u64::from(report.new_messages) != options.spec.messages {
        return Err(BenchError::Core(format!(
            "the initial sync stored {} of {} messages",
            report.new_messages, options.spec.messages
        )));
    }
    facts.messages = u64::from(report.new_messages);
    facts.folders = report.folders;
    facts.sync_messages_per_sec = f64_of(facts.messages) / initial.as_secs_f64().max(1e-9);
    metrics.push(Metric {
        metric: MetricName::InitialSyncSecs,
        value: initial.as_secs_f64(),
        samples: 1,
    });

    // --- Incremental sync -----------------------------------------------
    mock.deliver(ARRIVALS)?;
    let started = Instant::now();
    let report = service.sync_account(&account).await?;
    let incremental = started.elapsed();
    if u64::from(report.new_messages) != ARRIVALS {
        return Err(BenchError::Core(format!(
            "the incremental sync stored {} of {ARRIVALS} new messages",
            report.new_messages
        )));
    }
    metrics.push(Metric {
        metric: MetricName::IncrementalSyncMs,
        value: ms(incremental),
        samples: 1,
    });
    mock.stop().await;

    // --- List pages -------------------------------------------------------
    let mut all = folders(&service, &account).await?;
    all.sort_by_key(|f| std::cmp::Reverse(f.total));
    facts.largest_folder = all.first().map(|f| FolderFact {
        id: f.id.to_string(),
        path: f.path.clone(),
        total: f.total,
    });
    let mut list_ms = Vec::new();
    for (i, folder) in all.iter().take(3).enumerate() {
        let samples = options.list_samples / 3 + u32::from(i == 0) * (options.list_samples % 3);
        for offset in list_offsets(folder.total, samples, options.spec.seed + i as u64) {
            let started = Instant::now();
            let (total, got) = list_page(&service, folder, offset).await?;
            list_ms.push(ms(started.elapsed()));
            if total != folder.total || (got == 0 && total > 0) {
                return Err(BenchError::Core(format!(
                    "{}: page at {offset} has {got} of {total}",
                    folder.path
                )));
            }
        }
    }
    metrics.push(Metric {
        metric: MetricName::ListPageP95Ms,
        value: percentile(&list_ms, 95.0),
        samples: u32::try_from(list_ms.len()).unwrap_or(u32::MAX),
    });

    // --- Conversation pages (P2: the default list) ----------------------------
    let mut thread_ms = Vec::new();
    for (i, folder) in all.iter().take(3).enumerate() {
        let started = Instant::now();
        let (conversations, _) = thread_page(&service, folder, 0).await?;
        thread_ms.push(ms(started.elapsed()));
        let samples = options.list_samples / 3 + u32::from(i == 0) * (options.list_samples % 3);
        for offset in list_offsets(conversations, samples, options.spec.seed + i as u64) {
            let started = Instant::now();
            let (total, got) = thread_page(&service, folder, offset).await?;
            thread_ms.push(ms(started.elapsed()));
            if total != conversations || (got == 0 && total > 0) {
                return Err(BenchError::Core(format!(
                    "{}: conversation page at {offset} has {got} of {total}",
                    folder.path
                )));
            }
        }
    }
    metrics.push(Metric {
        metric: MetricName::ThreadPageP95Ms,
        value: percentile(&thread_ms, 95.0),
        samples: u32::try_from(thread_ms.len()).unwrap_or(u32::MAX),
    });

    // --- Search -----------------------------------------------------------
    let mut search_ms = Vec::new();
    for query in corpus.search_queries() {
        let mut rounds = Vec::new();
        let mut hits = 0;
        for _ in 0..options.search_rounds.max(1) {
            let started = Instant::now();
            hits = search(&service, &query).await?;
            rounds.push(ms(started.elapsed()));
        }
        search_ms.extend(&rounds);
        facts.queries.push(QueryFact {
            query,
            hits,
            max_ms: rounds.iter().copied().fold(0.0, f64::max),
            median_ms: percentile(&rounds, 50.0),
        });
    }
    if let Some(rare) = facts
        .queries
        .iter()
        .find(|q| q.query == crate::corpus::RARE_WORD)
        && rare.hits != 1
    {
        return Err(BenchError::Core(format!(
            "the rare word has {} hits instead of 1: the index is incomplete",
            rare.hits
        )));
    }
    metrics.push(Metric {
        metric: MetricName::SearchP95Ms,
        value: percentile(&search_ms, 95.0),
        samples: u32::try_from(search_ms.len()).unwrap_or(u32::MAX),
    });

    // --- Cold start ---------------------------------------------------------
    close(service).await;
    let started = Instant::now();
    let service = Service::open(service_options(&data_dir, &config_dir, &secrets))?;
    let inbox = folders(&service, &account)
        .await?
        .into_iter()
        .find(|f| f.role == FolderRole::Inbox)
        .ok_or_else(|| BenchError::Core("no Inbox after the restart".into()))?;
    let (_, got) = list_page(&service, &inbox, 0).await?;
    let cold = started.elapsed();
    if got == 0 {
        return Err(BenchError::Core(
            "the Inbox is empty after the restart".into(),
        ));
    }
    // The reopened service must have its persistent index back (a busy
    // index lock would leave it searching in memory, without the corpus).
    let rare = search(&service, crate::corpus::RARE_WORD).await?;
    if rare != 1 {
        return Err(BenchError::Core(format!(
            "after the restart the rare word has {rare} hits: the search index did not reopen"
        )));
    }
    metrics.push(Metric {
        metric: MetricName::ColdStartMs,
        value: ms(cold),
        samples: 1,
    });
    close(service).await;

    // The data is handed to `nail serve` (Playwright) as a local account:
    // the mock server is gone, and accounts without a configured server
    // show only when they are local.
    hand_over(&data_dir, &account, &ready.username)?;

    // --- Memory -------------------------------------------------------------
    if let Some(peak) = crate::memory::peak_working_set_mib() {
        metrics.push(Metric {
            metric: MetricName::PeakMemoryMib,
            value: peak,
            samples: 1,
        });
    }
    facts.data_dir_bytes = dir_size(&data_dir);
    Ok((metrics, facts))
}

fn hand_over(data_dir: &Path, account: &AccountId, email: &str) -> Result<(), BenchError> {
    let (store, _blobs) = nail_store::open_data_dir(data_dir)
        .map_err(|e| BenchError::Core(format!("reopening the store: {e}")))?;
    store
        .upsert_account(&Account {
            id: account.clone(),
            name: "Benchmark".into(),
            email: email.to_owned(),
            kind: AccountKind::Local,
        })
        .map_err(|e| BenchError::Core(format!("handing the account over: {e}")))?;
    Ok(())
}

fn imap_settings(ready: &crate::mock::MockReady) -> ImapAccount {
    ImapAccount {
        config: ImapConfig::new("127.0.0.1")
            .with_port(ready.port)
            .with_security(Security::InsecurePlaintext)
            .with_timeout(Duration::from_mins(2)),
        credentials: Credentials::new(&ready.username, &ready.password),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn percentiles_use_the_nearest_rank() {
        // The nearest rank is one of the samples, returned exactly: compare
        // the bits.
        let exact = |p: f64, want: f64| assert_eq!(p.to_bits(), want.to_bits(), "{p} != {want}");
        let samples: Vec<f64> = (1..=100).map(f64::from).collect();
        exact(percentile(&samples, 95.0), 95.0);
        exact(percentile(&samples, 50.0), 50.0);
        exact(percentile(&samples, 100.0), 100.0);
        exact(percentile(&samples, 0.0), 1.0);
        exact(percentile(&[3.0, 1.0, 2.0], 95.0), 3.0);
        exact(percentile(&[], 95.0), 0.0);
    }

    #[test]
    fn list_offsets_cover_the_top_and_the_rest() {
        let offsets = list_offsets(10_000, 40, 1);
        assert_eq!(offsets.len(), 40);
        assert!(offsets.iter().all(|o| o % PAGE_SIZE == 0 && *o < 10_000));
        assert_eq!(offsets[0], 0);
        assert_eq!(offsets[1], PAGE_SIZE);
        assert!(offsets.iter().any(|o| *o > 5_000));
        assert_eq!(list_offsets(10_000, 40, 1), offsets);
        assert_eq!(list_offsets(0, 4, 1), vec![0_u32; 4]);
    }
}
