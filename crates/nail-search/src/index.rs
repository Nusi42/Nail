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

use std::path::Path;

use nail_api::{AccountId, MessageId};
use tantivy::Term;
use tantivy::collector::TopDocs;
use tantivy::query::{
    AllQuery, BooleanQuery, BoostQuery, EmptyQuery, Occur, PhraseQuery, Query, TermQuery,
};
use tantivy::schema::{Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, Value};

use crate::document::IndexDocument;
pub use crate::engine::{CommitPolicy, DirectoryWrapper, VERSION_FILE_NAME};
use crate::engine::{Engine, IndexKind, text_options, tokens};
use crate::error::Result;
use crate::query::TextField;

/// Conventional name of the index directory inside the data directory.
pub const INDEX_DIR_NAME: &str = "search";

/// The mail index. Bump the version when the schema or tokenization
/// changes; older indexes are rebuilt. v2 (M2a): the `decrypted_in` marker
/// field.
const KIND: IndexKind = IndexKind {
    label: "search index",
    schema_version: 2,
    writer_memory: 50_000_000,
};

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
    let text = text_options();
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

/// The full-text index (one document per message, batched commits). `Send + Sync`; all
/// methods block (run them on a blocking thread from async code).
pub struct SearchIndex {
    engine: Engine,
    fields: Fields,
}

impl std::fmt::Debug for SearchIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SearchIndex")
            .field("path", &self.engine.path())
            .field("policy", &self.engine.policy())
            .finish_non_exhaustive()
    }
}

impl SearchIndex {
    index_api!();

    /// Adds (or replaces) documents.
    pub fn add(&self, docs: &[IndexDocument]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        {
            let writer = self.engine.writer();
            for doc in docs {
                writer.delete_term(Term::from_field_text(
                    self.fields.id,
                    doc.message_id.as_str(),
                ));
                writer.add_document(self.to_tantivy(doc))?;
            }
        }
        self.engine.staged(docs.len())
    }

    /// Removes documents (unknown ids are ignored).
    pub fn remove(&self, ids: &[MessageId]) -> Result<()> {
        if ids.is_empty() {
            return Ok(());
        }
        {
            let writer = self.engine.writer();
            for id in ids {
                writer.delete_term(Term::from_field_text(self.fields.id, id.as_str()));
            }
        }
        self.engine.staged(ids.len())
    }

    /// Removes every document of an account.
    pub fn remove_account(&self, account: &AccountId) -> Result<()> {
        self.engine
            .writer()
            .delete_term(Term::from_field_text(self.fields.account, account.as_str()));
        self.engine.staged(1)
    }

    /// Removes the documents of an account that were built from decrypted
    /// content (the opt-in was withdrawn); the caller re-adds those
    /// messages without their decrypted content.
    pub fn remove_decrypted(&self, account: &AccountId) -> Result<()> {
        self.engine.writer().delete_term(Term::from_field_text(
            self.fields.decrypted_in,
            account.as_str(),
        ));
        self.engine.staged(1)
    }

    /// Ids of an account's documents built from decrypted content.
    pub fn decrypted_ids(&self, account: &AccountId) -> Result<Vec<MessageId>> {
        let query = TermQuery::new(
            Term::from_field_text(self.fields.decrypted_in, account.as_str()),
            IndexRecordOption::Basic,
        );
        Ok(self.run(&query)?.into_iter().map(|(id, _)| id).collect())
    }

    /// Number of staged, uncommitted changes.
    pub fn pending(&self) -> usize {
        self.engine.pending()
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

    /// The Tantivy query for one text term: every token must appear (as a
    /// phrase when there are several) in one of the field's index fields.
    /// `None` when the text has no searchable tokens.
    pub(crate) fn text_query(&self, field: TextField, text: &str) -> Option<Box<dyn Query>> {
        let tokens = tokens(text);
        if tokens.is_empty() {
            return None;
        }
        let mut per_field: Vec<(Occur, Box<dyn Query>)> = self
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
        if per_field.len() == 1
            && let Some((_, only)) = per_field.pop()
        {
            return Some(only);
        }
        Some(Box::new(BooleanQuery::new(per_field)))
    }

    /// Runs a Tantivy query; returns every matching message with its score.
    pub(crate) fn run(&self, query: &dyn Query) -> Result<Vec<(MessageId, f32)>> {
        let searcher = self.engine.searcher()?;
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
    pub(crate) fn all_of(mut queries: Vec<Box<dyn Query>>) -> Box<dyn Query> {
        if queries.len() <= 1 {
            return queries.pop().unwrap_or_else(|| Box::new(AllQuery));
        }
        Box::new(BooleanQuery::new(
            queries.into_iter().map(|q| (Occur::Must, q)).collect(),
        ))
    }

    /// Text terms combined with OR.
    pub(crate) fn any_of(mut queries: Vec<Box<dyn Query>>) -> Box<dyn Query> {
        if queries.len() <= 1 {
            return queries.pop().unwrap_or_else(|| Box::new(EmptyQuery));
        }
        Box::new(BooleanQuery::new(
            queries.into_iter().map(|q| (Occur::Should, q)).collect(),
        ))
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

    use std::time::Duration;

    fn doc(id: &str, subject: &str, body: &str) -> IndexDocument {
        IndexDocument {
            message_id: id.into(),
            account_id: "a1".into(),
            from: "Ada Lovelace <ada@example.org>".into(),
            to: "Bob <bob@example.org>".into(),
            cc: String::new(),
            subject: subject.into(),
            body: body.into(),
            attachment_names: vec!["Rechnung-Menü.pdf".into()],
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
                .is_empty(),
            "{:?}",
            index.search_text(TextField::From, "bob").unwrap()
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
                .is_empty(),
            "{:?}",
            index.search_text(TextField::Body, "are numbers").unwrap()
        );
        // ASCII folding: Menü matches menu.
        assert_eq!(
            ids(index.search_text(TextField::Filename, "menu").unwrap()).len(),
            2
        );
        assert_eq!(index.search_text(TextField::Any, "!!!").unwrap().len(), 0);

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
        use std::sync::atomic::Ordering;
        let dir = tempfile::tempdir().unwrap();
        let (wrapper, calls) = crate::engine::counting_wrapper();
        let index =
            SearchIndex::open_with(dir.path(), CommitPolicy::default(), Some(&wrapper)).unwrap();
        index.add(&[doc("m1", "hello", "world")]).unwrap();
        assert_eq!(index.doc_count().unwrap(), 1);
        assert_eq!(calls.load(Ordering::SeqCst), 1);
    }

    #[test]
    fn commit_batching() {
        let policy = CommitPolicy {
            max_pending: 3,
            max_delay: Duration::from_hours(1),
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
        std::fs::write(path.join(VERSION_FILE_NAME), "0").unwrap();
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
