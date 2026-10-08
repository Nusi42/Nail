//! The Tantivy engine the mail index ([`crate::SearchIndex`]) and the PIM
//! index ([`crate::PimIndex`]) share (wave Q2, ADR 0171): the analyzer,
//! opening an index directory with a schema version file (rebuilt when the
//! version differs or the directory cannot be read, e.g. through another
//! encryption key), and batched commits by a [`CommitPolicy`].

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, MutexGuard, PoisonError};
use std::time::{Duration, Instant};

use tantivy::directory::Directory;
use tantivy::schema::{IndexRecordOption, Schema, TextFieldIndexing, TextOptions};
use tantivy::tokenizer::{
    AsciiFoldingFilter, LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, Searcher};

use crate::error::{Error, Result};

/// The schema version file in an index directory (plain; `nail-vault`
/// leaves it unencrypted).
pub const VERSION_FILE_NAME: &str = "nail-search-version";

/// Wraps the on-disk index directory, e.g. in `nail-vault`'s encrypting
/// directory.
pub type DirectoryWrapper = Arc<dyn Fn(Box<dyn Directory>) -> Box<dyn Directory> + Send + Sync>;

/// The name the analyzer is registered under.
pub(crate) const TOKENIZER: &str = "nail";

/// When staged changes are committed.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CommitPolicy {
    /// Commit once this many changes are staged.
    pub max_pending: usize,
    /// Commit once the oldest staged change is this old.
    pub max_delay: Duration,
}

impl Default for CommitPolicy {
    fn default() -> Self {
        Self {
            max_pending: 1_000,
            max_delay: Duration::from_secs(2),
        }
    }
}

/// Text is tokenized on non-alphanumerics, lowercased and ASCII-folded
/// (`Müller` matches `muller`); tokens longer than 64 bytes are dropped.
pub(crate) fn analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(64))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .build()
}

/// Tokens of `text` as the index sees them.
pub(crate) fn tokens(text: &str) -> Vec<String> {
    let mut analyzer = analyzer();
    let mut stream = analyzer.token_stream(text);
    let mut out = Vec::new();
    while let Some(token) = stream.next() {
        out.push(token.text.clone());
    }
    out
}

/// The options of an analyzed full-text field (positions for phrases).
pub(crate) fn text_options() -> TextOptions {
    TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    )
}

/// What tells one index from the other.
#[derive(Debug, Clone, Copy)]
pub(crate) struct IndexKind {
    /// For logs: `"search index"`, `"PIM index"`.
    pub(crate) label: &'static str,
    /// Bump when the schema or tokenization changes; older indexes are
    /// rebuilt.
    pub(crate) schema_version: u32,
    /// The writer's memory budget in bytes.
    pub(crate) writer_memory: usize,
}

#[derive(Debug, Default)]
struct Batch {
    pending: usize,
    since: Option<Instant>,
}

/// An open Tantivy index with batched commits.
pub(crate) struct Engine {
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    batch: Mutex<Batch>,
    policy: CommitPolicy,
    rebuilt: bool,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for Engine {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Engine")
            .field("path", &self.path)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl Engine {
    /// An index in memory (tests, demos, a locked vault).
    pub(crate) fn in_ram(schema: Schema, policy: CommitPolicy, kind: IndexKind) -> Result<Self> {
        Self::init(&Index::create_in_ram(schema), policy, kind, false, None)
    }

    /// Opens (or creates) the index in `dir`, its directory wrapped by
    /// `wrapper` (encryption at rest). An index of another schema version,
    /// or one that cannot be opened, is deleted and recreated empty
    /// ([`Self::rebuilt`]).
    pub(crate) fn open(
        dir: &Path,
        schema: &Schema,
        policy: CommitPolicy,
        wrapper: Option<&DirectoryWrapper>,
        kind: IndexKind,
    ) -> Result<Self> {
        let version_file = dir.join(VERSION_FILE_NAME);
        let current = std::fs::read_to_string(&version_file)
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        let mut rebuilt = false;
        if dir.exists() && current != Some(kind.schema_version) {
            tracing::info!(
                ?dir,
                ?current,
                index = kind.label,
                "another index version; rebuilding"
            );
            std::fs::remove_dir_all(dir)?;
            rebuilt = true;
        }
        std::fs::create_dir_all(dir)?;
        let index = match open_dir(dir, schema, wrapper) {
            Ok(index) => index,
            Err(err) => {
                tracing::warn!(?dir, %err, index = kind.label, "index unreadable; rebuilding");
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                rebuilt = true;
                open_dir(dir, schema, wrapper)?
            }
        };
        std::fs::write(&version_file, kind.schema_version.to_string())?;
        Self::init(&index, policy, kind, rebuilt, Some(dir.to_owned()))
    }

    fn init(
        index: &Index,
        policy: CommitPolicy,
        kind: IndexKind,
        rebuilt: bool,
        path: Option<PathBuf>,
    ) -> Result<Self> {
        index.tokenizers().register(TOKENIZER, analyzer());
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let writer = index.writer_with_num_threads(1, kind.writer_memory)?;
        Ok(Self {
            reader,
            writer: Mutex::new(writer),
            batch: Mutex::new(Batch::default()),
            policy,
            rebuilt,
            path,
        })
    }

    /// Whether opening recreated the index (the caller reindexes).
    pub(crate) fn rebuilt(&self) -> bool {
        self.rebuilt
    }

    /// The index directory (`None` in memory).
    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    /// The commit policy.
    pub(crate) fn policy(&self) -> CommitPolicy {
        self.policy
    }

    /// The writer, for staging changes ([`Self::staged`] after them).
    pub(crate) fn writer(&self) -> MutexGuard<'_, IndexWriter> {
        self.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn batch(&self) -> MutexGuard<'_, Batch> {
        self.batch.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn is_due(&self, batch: &Batch) -> bool {
        batch.pending >= self.policy.max_pending
            || batch
                .since
                .is_some_and(|t| t.elapsed() >= self.policy.max_delay)
    }

    /// Records `n` staged changes; commits when the policy says so.
    pub(crate) fn staged(&self, n: usize) -> Result<()> {
        let due = {
            let mut batch = self.batch();
            batch.pending += n;
            batch.since.get_or_insert_with(Instant::now);
            self.is_due(&batch)
        };
        if due {
            self.commit()?;
        }
        Ok(())
    }

    /// Commits staged changes now and makes them searchable.
    pub(crate) fn commit(&self) -> Result<()> {
        let mut batch = self.batch();
        self.writer().commit()?;
        self.reader.reload()?;
        *batch = Batch::default();
        Ok(())
    }

    /// Commits when the batch is due by the policy (call from a timer).
    /// Returns whether it committed.
    pub(crate) fn commit_if_due(&self) -> Result<bool> {
        let due = {
            let batch = self.batch();
            batch.pending > 0 && self.is_due(&batch)
        };
        if due {
            self.commit()?;
        }
        Ok(due)
    }

    /// Number of staged, uncommitted changes.
    pub(crate) fn pending(&self) -> usize {
        self.batch().pending
    }

    /// A searcher over every change (staged ones are committed first).
    pub(crate) fn searcher(&self) -> Result<Searcher> {
        if self.pending() > 0 {
            self.commit()?;
        }
        Ok(self.reader.searcher())
    }
}

/// The methods every index type has, one definition for the mail and the
/// PIM index (`impl X { index_api!(); }` in a module with `schema()`,
/// `KIND` and a struct `{ engine, fields }`): opening, commits, counts.
macro_rules! index_api {
    () => {
        /// An index in memory (tests, demos, a locked vault).
        pub fn open_in_ram(policy: CommitPolicy) -> Result<Self> {
            let (schema, fields) = schema();
            Ok(Self {
                engine: Engine::in_ram(schema, policy, KIND)?,
                fields,
            })
        }

        /// Opens (or creates) the index in `dir`. An index of another schema
        /// version, or one that cannot be opened, is deleted and recreated
        /// empty ([`Self::was_rebuilt`]).
        pub fn open(dir: impl AsRef<Path>, policy: CommitPolicy) -> Result<Self> {
            Self::open_with(dir, policy, None)
        }

        /// Like [`Self::open`], with the on-disk directory wrapped by
        /// `wrapper` (encryption at rest). An index the wrapper cannot read
        /// is rebuilt.
        pub fn open_with(
            dir: impl AsRef<Path>,
            policy: CommitPolicy,
            wrapper: Option<&DirectoryWrapper>,
        ) -> Result<Self> {
            let (schema, fields) = schema();
            Ok(Self {
                engine: Engine::open(dir.as_ref(), &schema, policy, wrapper, KIND)?,
                fields,
            })
        }

        /// Whether opening had to recreate the index (the caller reindexes).
        pub fn was_rebuilt(&self) -> bool {
            self.engine.rebuilt()
        }

        /// Removes every document (before a full reindex).
        pub fn clear(&self) -> Result<()> {
            self.engine.writer().delete_all_documents()?;
            self.engine.staged(1)
        }

        /// Commits staged changes now and makes them searchable.
        pub fn commit(&self) -> Result<()> {
            self.engine.commit()
        }

        /// Commits when the batch is due by the policy (call from a timer).
        /// Returns whether it committed.
        pub fn commit_if_due(&self) -> Result<bool> {
            self.engine.commit_if_due()
        }

        /// Number of searchable documents (after committing staged changes).
        pub fn doc_count(&self) -> Result<u64> {
            Ok(self.engine.searcher()?.num_docs())
        }
    };
}

/// The index in `dir` through `wrapper`.
fn open_dir(dir: &Path, schema: &Schema, wrapper: Option<&DirectoryWrapper>) -> Result<Index> {
    let directory: Box<dyn Directory> = Box::new(
        tantivy::directory::MmapDirectory::open(dir).map_err(|e| Error::Index(e.to_string()))?,
    );
    let directory = match wrapper {
        Some(wrap) => wrap(directory),
        None => directory,
    };
    Ok(Index::open_or_create(directory, schema.clone())?)
}

/// A wrapper that counts how often it wrapped a directory (tests).
#[cfg(test)]
pub(crate) fn counting_wrapper() -> (DirectoryWrapper, Arc<std::sync::atomic::AtomicUsize>) {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let calls = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&calls);
    let wrapper: DirectoryWrapper = Arc::new(move |inner| {
        counter.fetch_add(1, Ordering::SeqCst);
        inner
    });
    (wrapper, calls)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tantivy::schema::STRING;

    const KIND: IndexKind = IndexKind {
        label: "test index",
        schema_version: 7,
        writer_memory: 15_000_000,
    };

    fn schema() -> Schema {
        let mut b = Schema::builder();
        b.add_text_field("id", STRING);
        b.add_text_field("text", text_options());
        b.build()
    }

    #[test]
    fn analyzer_folds_and_splits() {
        assert_eq!(
            tokens("Müller-Lüdenscheidt ADA@x.org"),
            ["muller", "ludenscheidt", "ada", "x", "org"]
        );
        assert_eq!(tokens(&"a".repeat(65)), Vec::<String>::new());
    }

    #[test]
    fn batches_commit_by_count_and_on_demand() {
        let policy = CommitPolicy {
            max_pending: 2,
            max_delay: Duration::from_secs(3_600),
        };
        let engine = Engine::in_ram(schema(), policy, KIND).unwrap();
        assert!(!engine.rebuilt() && engine.path().is_none());
        assert_eq!(engine.policy(), policy);
        engine.staged(1).unwrap();
        assert_eq!(engine.pending(), 1);
        assert!(!engine.commit_if_due().unwrap());
        engine.staged(1).unwrap();
        assert_eq!(engine.pending(), 0, "the second change reached max_pending");
        engine.staged(1).unwrap();
        assert_eq!(engine.searcher().unwrap().num_docs(), 0);
        assert_eq!(engine.pending(), 0, "searching commits first");
    }

    #[test]
    fn another_version_or_an_unreadable_index_is_rebuilt() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("index");
        let first = Engine::open(&path, &schema(), CommitPolicy::default(), None, KIND).unwrap();
        assert!(!first.rebuilt());
        assert_eq!(first.path(), Some(path.as_path()));
        drop(first);
        let again = Engine::open(&path, &schema(), CommitPolicy::default(), None, KIND).unwrap();
        assert!(!again.rebuilt());
        drop(again);
        let newer = IndexKind {
            schema_version: 8,
            ..KIND
        };
        let rebuilt = Engine::open(&path, &schema(), CommitPolicy::default(), None, newer).unwrap();
        assert!(rebuilt.rebuilt());
        assert_eq!(
            std::fs::read_to_string(path.join(VERSION_FILE_NAME)).unwrap(),
            "8"
        );
    }
}
