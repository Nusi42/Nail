//! The full-text index (Tantivy) over headers, decoded body text and
//! attachment names.
//!
//! * One document per stored message, keyed by its [`MessageId`]; adding a
//!   message again replaces its document.
//! * Writes are batched: [`SearchIndex::add`] / [`SearchIndex::remove`]
//!   stage changes and commit once [`CommitPolicy::max_pending`] changes
//!   are staged or [`CommitPolicy::max_delay`] has passed since the first
//!   staged change (checked on every write and by
//!   [`SearchIndex::commit_if_due`], which the core calls from a timer).
//!   Searches commit staged changes first, so they are always visible.
//! * On disk the index lives in its own directory ([`INDEX_DIR_NAME`] in the
//!   data dir) with a schema version file; an index written with another
//!   schema version (or a corrupt one) is deleted and recreated empty, and
//!   [`SearchIndex::was_rebuilt`] tells the caller to reindex.
//! * Text is tokenized on non-alphanumerics, lowercased and ASCII-folded
//!   (`Müller` matches `muller`), so addresses match by their parts
//!   (`from:ada` finds `ada@example.org`).
//! * Encryption at rest (M2a WP33): [`SearchIndex::open_with`] takes a
//!   [`DirectoryWrapper`] (from `nail-vault`) that wraps the on-disk
//!   directory, e.g. in an encrypting one. An index that cannot be read
//!   through the wrapper (another key, plain files after encryption) is
//!   rebuilt like a corrupt one — it is derived data.
//! * Documents built from decrypted content are marked
//!   ([`IndexDocument::decrypted`]) so they can be dropped per account when
//!   the opt-in is withdrawn ([`SearchIndex::remove_decrypted`]; policy in
//!   [`crate::IndexPolicy`]).

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, Instant};

use nail_api::{AccountId, MessageId};
use tantivy::collector::TopDocs;
use tantivy::directory::Directory;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, EmptyQuery, Occur, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions, Value,
};
use tantivy::tokenizer::{
    AsciiFoldingFilter, LowerCaser, RemoveLongFilter, SimpleTokenizer, TextAnalyzer,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, Term};

use crate::document::IndexDocument;
use crate::error::{Error, Result};
use crate::query::TextField;

/// Conventional name of the index directory inside the data directory.
pub const INDEX_DIR_NAME: &str = "search";

/// Bump when the schema or tokenization changes; older indexes are rebuilt.
/// v2 (M2a): the `decrypted_in` marker field.
const SCHEMA_VERSION: u32 = 2;
/// The schema version file in the index directory (plain; `nail-vault`
/// leaves it unencrypted).
pub const VERSION_FILE_NAME: &str = "nail-search-version";
const VERSION_FILE: &str = VERSION_FILE_NAME;

/// Wraps the on-disk index directory, e.g. in `nail-vault`'s encrypting
/// directory.
pub type DirectoryWrapper = Arc<dyn Fn(Box<dyn Directory>) -> Box<dyn Directory> + Send + Sync>;
const TOKENIZER: &str = "nail";
const WRITER_MEMORY: usize = 50_000_000;

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

#[derive(Debug, Clone, Copy)]
struct Fields {
    id: Field,
    account: Field,
    from: Field,
    to: Field,
    cc: Field,
    subject: Field,
    body: Field,
    filename: Field,
    /// The account id, only on documents built from decrypted content.
    decrypted_in: Field,
}

impl Fields {
    fn text(&self, field: TextField) -> Vec<(Field, f32)> {
        match field {
            TextField::Any => vec![
                (self.subject, 2.0),
                (self.from, 1.5),
                (self.to, 1.0),
                (self.cc, 1.0),
                (self.body, 1.0),
                (self.filename, 1.0),
            ],
            TextField::From => vec![(self.from, 1.0)],
            TextField::To => vec![(self.to, 1.0)],
            TextField::Cc => vec![(self.cc, 1.0)],
            TextField::Subject => vec![(self.subject, 1.0)],
            TextField::Body => vec![(self.body, 1.0)],
            TextField::Filename => vec![(self.filename, 1.0)],
        }
    }
}

fn schema() -> (Schema, Fields) {
    let mut b = Schema::builder();
    let text = TextOptions::default().set_indexing_options(
        TextFieldIndexing::default()
            .set_tokenizer(TOKENIZER)
            .set_index_option(IndexRecordOption::WithFreqsAndPositions),
    );
    let fields = Fields {
        id: b.add_text_field("id", STRING | STORED),
        account: b.add_text_field("account", STRING),
        from: b.add_text_field("from", text.clone()),
        to: b.add_text_field("to", text.clone()),
        cc: b.add_text_field("cc", text.clone()),
        subject: b.add_text_field("subject", text.clone()),
        body: b.add_text_field("body", text.clone()),
        filename: b.add_text_field("filename", text),
        decrypted_in: b.add_text_field("decrypted_in", STRING),
    };
    (b.build(), fields)
}

fn analyzer() -> TextAnalyzer {
    TextAnalyzer::builder(SimpleTokenizer::default())
        .filter(RemoveLongFilter::limit(64))
        .filter(LowerCaser)
        .filter(AsciiFoldingFilter)
        .build()
}

#[derive(Debug, Default)]
struct Batch {
    pending: usize,
    since: Option<Instant>,
}

/// The full-text index (one document per message, batched commits). `Send + Sync`; all
/// methods block (run them on a blocking thread from async code).
pub struct SearchIndex {
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    batch: Mutex<Batch>,
    policy: CommitPolicy,
    rebuilt: bool,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchIndex")
            .field("path", &self.path)
            .field("policy", &self.policy)
            .finish_non_exhaustive()
    }
}

impl SearchIndex {
    /// An index in memory (tests, demos).
    pub fn open_in_ram(policy: CommitPolicy) -> Result<Self> {
        let (schema, fields) = schema();
        Self::init(Index::create_in_ram(schema), fields, policy, false, None)
    }

    /// Opens (or creates) the index in `dir`. An index of another schema
    /// version, or one that cannot be opened, is deleted and recreated
    /// empty ([`SearchIndex::was_rebuilt`]).
    pub fn open(dir: impl AsRef<Path>, policy: CommitPolicy) -> Result<Self> {
        Self::open_with(dir, policy, None)
    }

    /// Like [`SearchIndex::open`], with the directory wrapped by `wrapper`
    /// (encryption at rest). An index the wrapper cannot read is rebuilt.
    pub fn open_with(
        dir: impl AsRef<Path>,
        policy: CommitPolicy,
        wrapper: Option<DirectoryWrapper>,
    ) -> Result<Self> {
        let dir = dir.as_ref();
        let version_file = dir.join(VERSION_FILE);
        let current = std::fs::read_to_string(&version_file)
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        let mut rebuilt = false;
        if dir.exists() && current != Some(SCHEMA_VERSION) {
            tracing::info!(
                ?dir,
                ?current,
                "search index has another version; rebuilding"
            );
            std::fs::remove_dir_all(dir)?;
            rebuilt = true;
        }
        std::fs::create_dir_all(dir)?;
        let (schema, fields) = schema();
        let index = match Self::open_dir(dir, &schema, wrapper.as_ref()) {
            Ok(index) => index,
            Err(err) => {
                tracing::warn!(?dir, %err, "search index unreadable; rebuilding");
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                rebuilt = true;
                Self::open_dir(dir, &schema, wrapper.as_ref())?
            }
        };
        std::fs::write(&version_file, SCHEMA_VERSION.to_string())?;
        Self::init(index, fields, policy, rebuilt, Some(dir.to_owned()))
    }

    fn open_dir(dir: &Path, schema: &Schema, wrapper: Option<&DirectoryWrapper>) -> Result<Index> {
        let directory: Box<dyn Directory> = Box::new(
            tantivy::directory::MmapDirectory::open(dir)
                .map_err(|e| Error::Index(e.to_string()))?,
        );
        let directory = match wrapper {
            Some(wrap) => wrap(directory),
            None => directory,
        };
        Ok(Index::open_or_create(directory, schema.clone())?)
    }

    fn init(
        index: Index,
        fields: Fields,
        policy: CommitPolicy,
        rebuilt: bool,
        path: Option<PathBuf>,
    ) -> Result<Self> {
        index.tokenizers().register(TOKENIZER, analyzer());
        let reader = index
            .reader_builder()
            .reload_policy(ReloadPolicy::Manual)
            .try_into()?;
        let writer = index.writer_with_num_threads(1, WRITER_MEMORY)?;
        Ok(Self {
            reader,
            writer: Mutex::new(writer),
            fields,
            batch: Mutex::new(Batch::default()),
            policy,
            rebuilt,
            path,
        })
    }

    /// Whether [`SearchIndex::open`] had to recreate the index (the caller
    /// should reindex every message).
    pub fn was_rebuilt(&self) -> bool {
        self.rebuilt
    }

    fn writer(&self) -> std::sync::MutexGuard<'_, IndexWriter> {
        self.writer.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn staged(&self, n: usize) -> Result<()> {
        let due = {
            let mut batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
            batch.pending += n;
            batch.since.get_or_insert_with(Instant::now);
            self.is_due(&batch)
        };
        if due {
            self.commit()?;
        }
        Ok(())
    }

    fn is_due(&self, batch: &Batch) -> bool {
        batch.pending >= self.policy.max_pending
            || batch
                .since
                .is_some_and(|t| t.elapsed() >= self.policy.max_delay)
    }

    /// Adds (or replaces) documents.
    pub fn add(&self, docs: &[IndexDocument]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        {
            let writer = self.writer();
            for doc in docs {
                writer.delete_term(Term::from_field_text(
                    self.fields.id,
                    doc.message_id.as_str(),
                ));
                writer.add_document(self.to_tantivy(doc))?;
            }
        }
        self.staged(docs.len())
    }

    /// Removes documents (unknown ids are ignored).
    pub fn remove(&self, ids: &[MessageId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        {
            let writer = self.writer();
            for id in ids {
                writer.delete_term(Term::from_field_text(self.fields.id, id.as_str()));
            }
        }
        self.staged(ids.len())
    }

    /// Removes every document of an account.
    pub fn remove_account(&self, account: &AccountId) -> Result<()> {
        self.writer()
            .delete_term(Term::from_field_text(self.fields.account, account.as_str()));
        self.staged(1)
    }

    /// Removes the documents of an account that were built from decrypted
    /// content (the opt-in was withdrawn); the caller re-adds those
    /// messages without their decrypted content.
    pub fn remove_decrypted(&self, account: &AccountId) -> Result<()> {
        self.writer().delete_term(Term::from_field_text(
            self.fields.decrypted_in,
            account.as_str(),
        ));
        self.staged(1)
    }

    /// Ids of an account's documents built from decrypted content.
    pub fn decrypted_ids(&self, account: &AccountId) -> Result<Vec<MessageId>> {
        let query = TermQuery::new(
            Term::from_field_text(self.fields.decrypted_in, account.as_str()),
            IndexRecordOption::Basic,
        );
        Ok(self.run(&query)?.into_iter().map(|(id, _)| id).collect())
    }

    /// Removes every document (before a full reindex).
    pub fn clear(&self) -> Result<()> {
        self.writer().delete_all_documents()?;
        self.staged(1)
    }

    /// Commits staged changes now and makes them searchable.
    pub fn commit(&self) -> Result<()> {
        let mut batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
        self.writer().commit()?;
        self.reader.reload()?;
        *batch = Batch::default();
        Ok(())
    }

    /// Commits when the batch is due by the policy (call from a timer).
    /// Returns whether it committed.
    pub fn commit_if_due(&self) -> Result<bool> {
        let due = {
            let batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
            batch.pending > 0 && self.is_due(&batch)
        };
        if due {
            self.commit()?;
        }
        Ok(due)
    }

    /// Number of staged, uncommitted changes.
    pub fn pending(&self) -> usize {
        self.batch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending
    }

    fn commit_pending(&self) -> Result<()> {
        if self.pending() > 0 {
            self.commit()?;
        }
        Ok(())
    }

    /// Number of searchable documents (after committing staged changes).
    pub fn doc_count(&self) -> Result<u64> {
        self.commit_pending()?;
        Ok(self.reader.searcher().num_docs())
    }

    fn to_tantivy(&self, doc: &IndexDocument) -> TantivyDocument {
        let f = &self.fields;
        let mut out = TantivyDocument::default();
        out.add_text(f.id, doc.message_id.as_str());
        out.add_text(f.account, doc.account_id.as_str());
        out.add_text(f.from, &doc.from);
        out.add_text(f.to, &doc.to);
        out.add_text(f.cc, &doc.cc);
        out.add_text(f.subject, &doc.subject);
        out.add_text(f.body, &doc.body);
        for name in &doc.attachment_names {
            out.add_text(f.filename, name);
        }
        if doc.decrypted {
            out.add_text(f.decrypted_in, doc.account_id.as_str());
        }
        out
    }

    /// Tokens of `text` as the index sees them.
    fn tokens(&self, text: &str) -> Vec<String> {
        let mut analyzer = analyzer();
        let mut stream = analyzer.token_stream(text);
        let mut out = Vec::new();
        while let Some(token) = stream.next() {
            out.push(token.text.clone());
        }
        out
    }

    /// The Tantivy query for one text term: every token must appear (as a
    /// phrase when there are several) in one of the field's index fields.
    /// `None` when the text has no searchable tokens.
    pub(crate) fn text_query(&self, field: TextField, text: &str) -> Option<Box<dyn Query>> {
        let tokens = self.tokens(text);
        if tokens.is_empty() {
            return None;
        }
        let per_field: Vec<(Occur, Box<dyn Query>)> = self
            .fields
            .text(field)
            .into_iter()
            .map(|(f, boost)| {
                let q: Box<dyn Query> = if tokens.len() == 1 {
                    Box::new(TermQuery::new(
                        Term::from_field_text(f, &tokens[0]),
                        IndexRecordOption::WithFreqs,
                    ))
                } else {
                    Box::new(PhraseQuery::new(
                        tokens.iter().map(|t| Term::from_field_text(f, t)).collect(),
                    ))
                };
                let q: Box<dyn Query> = if (boost - 1.0).abs() > f32::EPSILON {
                    Box::new(BoostQuery::new(q, boost))
                } else {
                    q
                };
                (Occur::Should, q)
            })
            .collect();
        Some(if per_field.len() == 1 {
            per_field
                .into_iter()
                .next()
                .map(|(_, q)| q)
                .expect("one field")
        } else {
            Box::new(BooleanQuery::new(per_field))
        })
    }

    /// Runs a Tantivy query; returns every matching message with its score.
    pub(crate) fn run(&self, query: &dyn Query) -> Result<Vec<(MessageId, f32)>> {
        self.commit_pending()?;
        let searcher = self.reader.searcher();
        let total = usize::try_from(searcher.num_docs()).unwrap_or(usize::MAX);
        if total == 0 {
            return Ok(Vec::new());
        }
        let hits = searcher.search(query, &TopDocs::with_limit(total).order_by_score())?;
        let mut out = Vec::with_capacity(hits.len());
        for (score, address) in hits {
            let doc: TantivyDocument = searcher.doc(address)?;
            if let Some(id) = doc.get_first(self.fields.id).and_then(|v| v.as_str()) {
                out.push((MessageId(id.to_owned()), score));
            }
        }
        Ok(out)
    }

    /// All text terms combined with AND (a query that matches nothing
    /// when a term has no tokens is avoided by the caller).
    pub(crate) fn all_of(queries: Vec<Box<dyn Query>>) -> Box<dyn Query> {
        match queries.len() {
            0 => Box::new(AllQuery),
            1 => queries.into_iter().next().expect("one query"),
            _ => Box::new(BooleanQuery::new(
                queries.into_iter().map(|q| (Occur::Must, q)).collect(),
            )),
        }
    }

    /// Text terms combined with OR.
    pub(crate) fn any_of(queries: Vec<Box<dyn Query>>) -> Box<dyn Query> {
        match queries.len() {
            0 => Box::new(EmptyQuery),
            1 => queries.into_iter().next().expect("one query"),
            _ => Box::new(BooleanQuery::new(
                queries.into_iter().map(|q| (Occur::Should, q)).collect(),
            )),
        }
    }

    /// Message ids matching one full-text term, with scores (e.g. for
    /// highlighting or rules).
    pub fn search_text(&self, field: TextField, text: &str) -> Result<Vec<(MessageId, f32)>> {
        match self.text_query(field, text) {
            Some(q) => self.run(q.as_ref()),
            None => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(id: &str, subject: &str, body: &str) -> IndexDocument {
        IndexDocument {
            message_id: id.into(),
            account_id: "a1".into(),
            from: "Ada Lovelace <ada@example.org>".into(),
            to: "Bob <bob@example.org>".into(),
            cc: String::new(),
            subject: subject.into(),
            body: body.into(),
            attachment_names: vec!["Rechnung-März.pdf".into()],
            decrypted: false,
        }
    }

    fn ids(hits: Vec<(MessageId, f32)>) -> Vec<String> {
        let mut v: Vec<String> = hits.into_iter().map(|(id, _)| id.0).collect();
        v.sort();
        v
    }

    #[test]
    fn add_replace_remove_and_search() {
        let index = SearchIndex::open_in_ram(CommitPolicy::default()).unwrap();
        index
            .add(&[
                doc("m1", "Quarterly report", "numbers inside"),
                doc("m2", "Lunch", "the quarterly numbers are late"),
            ])
            .unwrap();
        assert_eq!(index.pending(), 2);
        assert_eq!(index.doc_count().unwrap(), 2, "reads commit staged changes");
        assert_eq!(index.pending(), 0);

        assert_eq!(
            ids(index.search_text(TextField::Any, "QUARTERLY").unwrap()),
            ["m1", "m2"]
        );
        assert_eq!(
            ids(index.search_text(TextField::Subject, "quarterly").unwrap()),
            ["m1"]
        );
        assert_eq!(
            ids(index
                .search_text(TextField::From, "ada@example.org")
                .unwrap())
            .len(),
            2
        );
        assert_eq!(
            ids(index.search_text(TextField::From, "lovelace").unwrap()).len(),
            2
        );
        assert!(
            index
                .search_text(TextField::From, "bob")
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            ids(index.search_text(TextField::To, "bob").unwrap()).len(),
            2
        );
        // Phrases need adjacent words.
        assert_eq!(
            ids(index.search_text(TextField::Body, "numbers are").unwrap()),
            ["m2"]
        );
        assert!(
            index
                .search_text(TextField::Body, "are numbers")
                .unwrap()
                .is_empty()
        );
        // ASCII folding: März matches marz.
        assert_eq!(
            ids(index.search_text(TextField::Filename, "marz").unwrap()).len(),
            2
        );
        assert!(index.search_text(TextField::Any, "!!!").unwrap().is_empty());

        // Subject hits rank above body hits.
        let ranked = index.search_text(TextField::Any, "quarterly").unwrap();
        assert_eq!(ranked[0].0, MessageId::from("m1"));

        // Re-adding replaces.
        index.add(&[doc("m1", "Renamed", "nothing")]).unwrap();
        assert_eq!(index.doc_count().unwrap(), 2);
        assert_eq!(
            ids(index.search_text(TextField::Subject, "quarterly").unwrap()),
            Vec::<String>::new()
        );
        assert_eq!(
            ids(index.search_text(TextField::Subject, "renamed").unwrap()),
            ["m1"]
        );

        index.remove(&["m1".into(), "unknown".into()]).unwrap();
        assert_eq!(index.doc_count().unwrap(), 1);
        index.remove_account(&"a1".into()).unwrap();
        assert_eq!(index.doc_count().unwrap(), 0);
    }

    #[test]
    fn decrypted_documents_are_marked_and_removed_per_account() {
        let index = SearchIndex::open_in_ram(CommitPolicy::default()).unwrap();
        let mut secret = doc("m1", "Protected", "decrypted launch plan");
        secret.decrypted = true;
        let mut other = doc("m2", "Protected too", "decrypted elsewhere");
        other.decrypted = true;
        other.account_id = "a2".into();
        index
            .add(&[secret, other, doc("m3", "Plain", "launch plan")])
            .unwrap();
        assert_eq!(
            index.decrypted_ids(&"a1".into()).unwrap(),
            vec![MessageId::from("m1")]
        );
        assert_eq!(
            index.decrypted_ids(&"a2".into()).unwrap(),
            vec![MessageId::from("m2")]
        );
        index.remove_decrypted(&"a1".into()).unwrap();
        assert_eq!(
            ids(index.search_text(TextField::Body, "launch").unwrap()),
            ["m3"]
        );
        assert_eq!(index.doc_count().unwrap(), 2);
    }

    #[test]
    fn wrapper_is_used_for_the_on_disk_directory() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let wrapper: DirectoryWrapper = Arc::new(move |inner| {
            counter.fetch_add(1, Ordering::SeqCst);
            inner
        });
        let index =
            SearchIndex::open_with(dir.path(), CommitPolicy::default(), Some(wrapper)).unwrap();
        index.add(&[doc("m1", "hello", "world")]).unwrap();
        assert_eq!(index.doc_count().unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn commit_batching() {
        let policy = CommitPolicy {
            max_pending: 3,
            max_delay: Duration::from_secs(3600),
        };
        let index = SearchIndex::open_in_ram(policy).unwrap();
        index
            .add(&[doc("m1", "a", "b"), doc("m2", "a", "b")])
            .unwrap();
        assert_eq!(index.pending(), 2);
        assert!(!index.commit_if_due().unwrap());
        index.add(&[doc("m3", "a", "b")]).unwrap();
        assert_eq!(index.pending(), 0, "committed at max_pending");

        let policy = CommitPolicy {
            max_pending: 1_000,
            max_delay: Duration::ZERO,
        };
        let index = SearchIndex::open_in_ram(policy).unwrap();
        index.add(&[doc("m1", "a", "b")]).unwrap();
        assert_eq!(index.pending(), 0, "committed at once with a zero delay");

        let policy = CommitPolicy {
            max_pending: 1_000,
            max_delay: Duration::from_millis(30),
        };
        let index = SearchIndex::open_in_ram(policy).unwrap();
        index.add(&[doc("m1", "a", "b")]).unwrap();
        assert!(!index.commit_if_due().unwrap());
        std::thread::sleep(Duration::from_millis(40));
        assert!(index.commit_if_due().unwrap());
        assert_eq!(index.pending(), 0);
    }

    #[test]
    fn on_disk_index_persists_and_rebuilds_on_version_change() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(INDEX_DIR_NAME);
        {
            let index = SearchIndex::open(&path, CommitPolicy::default()).unwrap();
            assert!(!index.was_rebuilt());
            index.add(&[doc("m1", "hello", "world")]).unwrap();
            index.commit().unwrap();
        }
        {
            let index = SearchIndex::open(&path, CommitPolicy::default()).unwrap();
            assert!(!index.was_rebuilt());
            assert_eq!(index.doc_count().unwrap(), 1);
            assert_eq!(
                ids(index.search_text(TextField::Any, "hello").unwrap()),
                ["m1"]
            );
        }
        std::fs::write(path.join(VERSION_FILE), "0").unwrap();
        let index = SearchIndex::open(&path, CommitPolicy::default()).unwrap();
        assert!(index.was_rebuilt());
        assert_eq!(index.doc_count().unwrap(), 0);

        // A corrupt index is rebuilt too.
        drop(index);
        std::fs::write(path.join("meta.json"), "not json").unwrap();
        let index = SearchIndex::open(&path, CommitPolicy::default()).unwrap();
        assert!(index.was_rebuilt());
        index.clear().unwrap();
        assert_eq!(index.doc_count().unwrap(), 0);
    }
}
