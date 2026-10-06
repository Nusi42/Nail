//! PIM search (M3a WP73; contract ADR 0058, ADR 0061): a Tantivy
//! index of events, tasks, contacts and groups behind
//! `nail_api::Request::SearchPim` and `SearchContacts`.
//!
//! * Its **own index** ([`PimIndex`], directory [`PIM_INDEX_DIR_NAME`] next
//!   to the mail index) with its own schema version, so PIM changes never
//!   force a mail reindex and the two can be rebuilt independently.
//! * **Encryption at rest like mail**: [`PimIndex::open_with`] takes the
//!   same [`DirectoryWrapper`] (`nail-vault`'s sealed directory); an index
//!   the wrapper cannot read is rebuilt ([`PimIndex::was_rebuilt`]: the
//!   core reindexes from the store, where PIM data is sealed too).
//! * One document per item, keyed by kind and id; fields: title (summary,
//!   display name, group name), names (people: organizer, attendees, name
//!   parts, nicknames, organization), email addresses, phone numbers
//!   (also as digits), location (event / task location, postal addresses)
//!   and other text (descriptions, notes, categories, custom fields).
//! * Queries are typed as you go: every word must match the start of a
//!   word in some field (`ad lovel` finds Ada Lovelace), titles and names
//!   rank first; a query of digits also matches phone numbers by their
//!   digits (`030 123` finds `+49 30 1234567`). Text is lower-cased and
//!   ASCII-folded like mail (`Müller` matches `muller`).
//! * Changes are staged and committed by the [`CommitPolicy`] (searches
//!   commit first, so they always see them).

use std::path::{Path, PathBuf};
use std::sync::{Mutex, PoisonError};
use std::time::Instant;

use nail_api::pim::contact::ContactMatchField;
use nail_api::pim::{
    CalendarEvent, Contact, ContactGroup, EventFields, PimItemKind, PimItemRef, Task,
};
use nail_api::{AddressBookId, ContactId};
use tantivy::collector::TopDocs;
use tantivy::directory::Directory;
use tantivy::query::{BooleanQuery, BoostQuery, FuzzyTermQuery, Occur, Query, TermQuery};
use tantivy::schema::{
    Field, IndexRecordOption, STORED, STRING, Schema, TantivyDocument, TextFieldIndexing,
    TextOptions, Value,
};
use tantivy::{Index, IndexReader, IndexWriter, ReloadPolicy, Term};

use crate::error::{Error, Result};
use crate::index::{CommitPolicy, DirectoryWrapper, TOKENIZER, VERSION_FILE_NAME, analyzer};

/// The item kinds PIM search covers.
pub const PIM_KINDS: [PimItemKind; 3] =
    [PimItemKind::Event, PimItemKind::Task, PimItemKind::Contact];

/// Conventional name of the PIM index directory inside the data directory.
pub const PIM_INDEX_DIR_NAME: &str = "search-pim";

/// Bump when the schema or tokenization changes; older indexes are rebuilt.
const SCHEMA_VERSION: u32 = 1;
const WRITER_MEMORY: usize = 15_000_000;

/// What one item contributes to the index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PimDocument {
    /// The item.
    pub item: PimItemRef,
    /// Its calendar or address book.
    pub collection_id: String,
    /// Summary, display name or group name.
    pub title: String,
    /// People's names, nicknames, organization.
    pub names: Vec<String>,
    /// Email addresses.
    pub emails: Vec<String>,
    /// Phone numbers.
    pub phones: Vec<String>,
    /// Locations and postal addresses.
    pub location: Vec<String>,
    /// Descriptions, notes, categories, other fields.
    pub text: Vec<String>,
}

fn push(out: &mut Vec<String>, value: Option<&str>) {
    if let Some(v) = value.map(str::trim).filter(|v| !v.is_empty()) {
        out.push(v.to_owned());
    }
}

fn event_fields(fields: &EventFields, doc: &mut PimDocument) {
    push(&mut doc.location, fields.location.as_deref());
    push(&mut doc.text, fields.description.as_deref());
    push(&mut doc.text, fields.url.as_deref());
    doc.text.extend(fields.categories.iter().cloned());
    if let Some(organizer) = &fields.organizer {
        push(&mut doc.names, organizer.name.as_deref());
        doc.emails.push(organizer.email.clone());
    }
    for attendee in &fields.attendees {
        push(&mut doc.names, attendee.name.as_deref());
        doc.emails.push(attendee.email.clone());
    }
    for c in &fields.conferences {
        push(&mut doc.text, c.label.as_deref());
    }
    for a in &fields.attachments {
        push(&mut doc.text, a.name.as_deref());
    }
}

/// The digits of a phone number and its last nine (as `nail-store`
/// compares them), leading zeros dropped.
fn phone_digit_forms(phone: &str) -> Vec<String> {
    let digits: String = phone.chars().filter(char::is_ascii_digit).collect();
    let digits = digits.trim_start_matches('0').to_owned();
    if digits.is_empty() {
        return Vec::new();
    }
    let mut out = vec![digits.clone()];
    if digits.len() > 9 {
        out.push(
            digits[digits.len() - 9..]
                .trim_start_matches('0')
                .to_owned(),
        );
    }
    out
}

impl PimDocument {
    fn empty(item: PimItemRef, collection_id: &str, title: &str) -> Self {
        Self {
            item,
            collection_id: collection_id.to_owned(),
            title: title.to_owned(),
            names: Vec::new(),
            emails: Vec::new(),
            phones: Vec::new(),
            location: Vec::new(),
            text: Vec::new(),
        }
    }

    /// An event series (overrides' titles and places count too).
    pub fn event(event: &CalendarEvent) -> Self {
        let mut doc = Self::empty(
            PimItemRef::Event {
                event_id: event.id.clone(),
            },
            event.calendar_id.as_str(),
            &event.fields.title,
        );
        event_fields(&event.fields, &mut doc);
        for o in &event.overrides {
            if o.fields.title != event.fields.title {
                push(&mut doc.text, Some(&o.fields.title));
            }
            event_fields(&o.fields, &mut doc);
        }
        doc.dedup();
        doc
    }

    /// A task.
    pub fn task(task: &Task) -> Self {
        let mut doc = Self::empty(
            PimItemRef::Task {
                task_id: task.id.clone(),
            },
            task.calendar_id.as_str(),
            &task.fields.title,
        );
        push(&mut doc.location, task.fields.location.as_deref());
        push(&mut doc.text, task.fields.description.as_deref());
        push(&mut doc.text, task.fields.url.as_deref());
        doc.text.extend(task.fields.categories.iter().cloned());
        doc
    }

    /// A contact.
    pub fn contact(contact: &Contact) -> Self {
        let f = &contact.fields;
        let mut doc = Self::empty(
            PimItemRef::Contact {
                contact_id: contact.id.clone(),
            },
            contact.address_book_id.as_str(),
            &f.display_name,
        );
        if let Some(name) = &f.name {
            for part in [
                &name.prefix,
                &name.given,
                &name.additional,
                &name.family,
                &name.suffix,
            ] {
                push(&mut doc.names, part.as_deref());
            }
        }
        doc.names.extend(f.nicknames.iter().cloned());
        if let Some(org) = &f.organization {
            push(&mut doc.names, Some(&org.name));
            doc.names.extend(org.units.iter().cloned());
        }
        push(&mut doc.names, f.title.as_deref());
        push(&mut doc.names, f.role.as_deref());
        doc.emails.extend(f.emails.iter().map(|e| e.value.clone()));
        for phone in &f.phones {
            doc.phones.push(phone.value.clone());
            doc.phones.extend(phone_digit_forms(&phone.value));
        }
        for a in &f.addresses {
            for part in [
                &a.street,
                &a.extended,
                &a.locality,
                &a.region,
                &a.postal_code,
                &a.country,
                &a.po_box,
            ] {
                push(&mut doc.location, part.as_deref());
            }
        }
        push(&mut doc.text, f.notes.as_deref());
        doc.text.extend(f.categories.iter().cloned());
        doc.text.extend(f.custom.iter().map(|c| c.value.clone()));
        for list in [&f.urls, &f.impp, &f.related] {
            doc.text.extend(list.iter().map(|v| v.value.clone()));
        }
        doc.dedup();
        doc
    }

    /// A contact group (its name and member addresses).
    pub fn group(group: &ContactGroup) -> Self {
        let mut doc = Self::empty(
            PimItemRef::ContactGroup {
                group_id: group.id.clone(),
            },
            group.address_book_id.as_str(),
            &group.name,
        );
        doc.emails.extend(group.member_emails.iter().cloned());
        doc
    }

    fn dedup(&mut self) {
        for list in [
            &mut self.names,
            &mut self.emails,
            &mut self.phones,
            &mut self.location,
            &mut self.text,
        ] {
            let mut seen = std::collections::HashSet::new();
            list.retain(|v| seen.insert(v.to_lowercase()));
        }
    }
}

fn kind_str(kind: PimItemKind) -> &'static str {
    match kind {
        PimItemKind::Event => "event",
        PimItemKind::Task => "task",
        PimItemKind::Contact => "contact",
        PimItemKind::ContactGroup => "group",
    }
}

fn item_key(item: &PimItemRef) -> String {
    let id = match item {
        PimItemRef::Event { event_id } => event_id.as_str(),
        PimItemRef::Task { task_id } => task_id.as_str(),
        PimItemRef::Contact { contact_id } => contact_id.as_str(),
        PimItemRef::ContactGroup { group_id } => group_id.as_str(),
    };
    format!("{}:{id}", kind_str(item.kind()))
}

fn item_of_key(key: &str) -> Option<PimItemRef> {
    let (kind, id) = key.split_once(':')?;
    Some(match kind {
        "event" => PimItemRef::Event {
            event_id: id.into(),
        },
        "task" => PimItemRef::Task { task_id: id.into() },
        "contact" => PimItemRef::Contact {
            contact_id: id.into(),
        },
        "group" => PimItemRef::ContactGroup {
            group_id: id.into(),
        },
        _ => return None,
    })
}

#[derive(Debug, Clone, Copy)]
struct Fields {
    key: Field,
    kind: Field,
    collection: Field,
    title: Field,
    names: Field,
    emails: Field,
    phones: Field,
    location: Field,
    text: Field,
}

impl Fields {
    fn weighted(&self) -> [(Field, f32); 6] {
        [
            (self.title, 3.0),
            (self.names, 2.0),
            (self.emails, 2.0),
            (self.phones, 1.5),
            (self.location, 1.0),
            (self.text, 0.8),
        ]
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
        key: b.add_text_field("key", STRING | STORED),
        kind: b.add_text_field("kind", STRING),
        collection: b.add_text_field("collection", STRING),
        title: b.add_text_field("title", text.clone()),
        names: b.add_text_field("names", text.clone()),
        emails: b.add_text_field("emails", text.clone()),
        phones: b.add_text_field("phones", text.clone()),
        location: b.add_text_field("location", text.clone()),
        text: b.add_text_field("text", text),
    };
    (b.build(), fields)
}

/// Tokens of `text` as the index sees them.
fn tokens(text: &str) -> Vec<String> {
    let mut analyzer = analyzer();
    let mut stream = analyzer.token_stream(text);
    let mut out = Vec::new();
    while let Some(token) = stream.next() {
        out.push(token.text.clone());
    }
    out
}

#[derive(Debug, Default)]
struct Batch {
    pending: usize,
    since: Option<Instant>,
}

/// The PIM search index. `Send + Sync`; methods block (use a blocking
/// thread from async code).
pub struct PimIndex {
    reader: IndexReader,
    writer: Mutex<IndexWriter>,
    fields: Fields,
    batch: Mutex<Batch>,
    policy: CommitPolicy,
    rebuilt: bool,
    path: Option<PathBuf>,
}

impl std::fmt::Debug for PimIndex {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PimIndex")
            .field("path", &self.path)
            .finish_non_exhaustive()
    }
}

impl PimIndex {
    /// An index in memory (tests, demos, a locked vault).
    pub fn open_in_ram(policy: CommitPolicy) -> Result<Self> {
        let (schema, fields) = schema();
        Self::init(&Index::create_in_ram(schema), fields, policy, false, None)
    }

    /// Opens (or creates) the index in `dir`; one of another schema
    /// version, or one that cannot be opened, is recreated empty.
    pub fn open(dir: impl AsRef<Path>, policy: CommitPolicy) -> Result<Self> {
        Self::open_with(dir, policy, None)
    }

    /// Like [`PimIndex::open`] with the on-disk directory wrapped
    /// (encryption at rest, as the mail index).
    pub fn open_with(
        dir: impl AsRef<Path>,
        policy: CommitPolicy,
        wrapper: Option<&DirectoryWrapper>,
    ) -> Result<Self> {
        let dir = dir.as_ref();
        let version_file = dir.join(VERSION_FILE_NAME);
        let current = std::fs::read_to_string(&version_file)
            .ok()
            .and_then(|v| v.trim().parse::<u32>().ok());
        let mut rebuilt = false;
        if dir.exists() && current != Some(SCHEMA_VERSION) {
            tracing::info!(?dir, ?current, "PIM index has another version; rebuilding");
            std::fs::remove_dir_all(dir)?;
            rebuilt = true;
        }
        std::fs::create_dir_all(dir)?;
        let (schema, fields) = schema();
        let open = |wrapper: Option<&DirectoryWrapper>| -> Result<Index> {
            let directory: Box<dyn Directory> = Box::new(
                tantivy::directory::MmapDirectory::open(dir)
                    .map_err(|e| Error::Index(e.to_string()))?,
            );
            let directory = match wrapper {
                Some(wrap) => wrap(directory),
                None => directory,
            };
            Ok(Index::open_or_create(directory, schema.clone())?)
        };
        let index = match open(wrapper) {
            Ok(index) => index,
            Err(err) => {
                tracing::warn!(?dir, %err, "PIM index unreadable; rebuilding");
                std::fs::remove_dir_all(dir)?;
                std::fs::create_dir_all(dir)?;
                rebuilt = true;
                open(wrapper)?
            }
        };
        std::fs::write(&version_file, SCHEMA_VERSION.to_string())?;
        Self::init(&index, fields, policy, rebuilt, Some(dir.to_owned()))
    }

    fn init(
        index: &Index,
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

    /// Whether opening recreated the index (the caller reindexes).
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
            batch.pending >= self.policy.max_pending
                || batch
                    .since
                    .is_some_and(|t| t.elapsed() >= self.policy.max_delay)
        };
        if due {
            self.commit()?;
        }
        Ok(())
    }

    /// Adds (or replaces) documents.
    pub fn add(&self, docs: &[PimDocument]) -> Result<()> {
        if docs.is_empty() {
            return Ok(());
        }
        {
            let writer = self.writer();
            let f = &self.fields;
            for doc in docs {
                let key = item_key(&doc.item);
                writer.delete_term(Term::from_field_text(f.key, &key));
                let mut out = TantivyDocument::default();
                out.add_text(f.key, &key);
                out.add_text(f.kind, kind_str(doc.item.kind()));
                out.add_text(f.collection, &doc.collection_id);
                out.add_text(f.title, &doc.title);
                for (field, values) in [
                    (f.names, &doc.names),
                    (f.emails, &doc.emails),
                    (f.phones, &doc.phones),
                    (f.location, &doc.location),
                    (f.text, &doc.text),
                ] {
                    for v in values {
                        out.add_text(field, v);
                    }
                }
                writer.add_document(out)?;
            }
        }
        self.staged(docs.len())
    }

    /// Removes items (unknown ones are ignored).
    pub fn remove(&self, items: &[PimItemRef]) -> Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        {
            let writer = self.writer();
            for item in items {
                writer.delete_term(Term::from_field_text(self.fields.key, &item_key(item)));
            }
        }
        self.staged(items.len())
    }

    /// Removes every item of a calendar or address book.
    pub fn remove_collection(&self, collection_id: &str) -> Result<()> {
        self.writer()
            .delete_term(Term::from_field_text(self.fields.collection, collection_id));
        self.staged(1)
    }

    /// Removes everything (before a full reindex).
    pub fn clear(&self) -> Result<()> {
        self.writer().delete_all_documents()?;
        self.staged(1)
    }

    /// Commits staged changes now.
    pub fn commit(&self) -> Result<()> {
        let mut batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
        self.writer().commit()?;
        self.reader.reload()?;
        *batch = Batch::default();
        Ok(())
    }

    /// Commits when due by the policy (call from a timer). Returns whether
    /// it committed.
    pub fn commit_if_due(&self) -> Result<bool> {
        let due = {
            let batch = self.batch.lock().unwrap_or_else(PoisonError::into_inner);
            batch.pending > 0
                && (batch.pending >= self.policy.max_pending
                    || batch
                        .since
                        .is_some_and(|t| t.elapsed() >= self.policy.max_delay))
        };
        if due {
            self.commit()?;
        }
        Ok(due)
    }

    fn commit_pending(&self) -> Result<()> {
        let pending = self
            .batch
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .pending;
        if pending > 0 {
            self.commit()?;
        }
        Ok(())
    }

    /// Number of searchable documents.
    pub fn doc_count(&self) -> Result<u64> {
        self.commit_pending()?;
        Ok(self.reader.searcher().num_docs())
    }

    /// The query for free text; `None` without searchable words.
    fn text_query(&self, query: &str) -> Option<Box<dyn Query>> {
        let words = tokens(query);
        if words.is_empty() {
            return None;
        }
        let per_word: Vec<(Occur, Box<dyn Query>)> = words
            .iter()
            .map(|word| {
                let alternatives: Vec<(Occur, Box<dyn Query>)> = self
                    .fields
                    .weighted()
                    .into_iter()
                    .flat_map(|(field, boost)| {
                        let term = Term::from_field_text(field, word);
                        let exact: Box<dyn Query> = Box::new(BoostQuery::new(
                            Box::new(TermQuery::new(term.clone(), IndexRecordOption::WithFreqs)),
                            boost * 1.5,
                        ));
                        let prefix: Box<dyn Query> = Box::new(BoostQuery::new(
                            Box::new(FuzzyTermQuery::new_prefix(term, 0, true)),
                            boost,
                        ));
                        [(Occur::Should, exact), (Occur::Should, prefix)]
                    })
                    .collect();
                let any: Box<dyn Query> = Box::new(BooleanQuery::new(alternatives));
                (Occur::Must, any)
            })
            .collect();
        let words_query: Box<dyn Query> = Box::new(BooleanQuery::new(per_word));
        // Digits only (spaces, +, -, / allowed): also as one phone number.
        let is_phone = query.chars().any(|c| c.is_ascii_digit())
            && query.chars().all(|c| {
                c.is_ascii_digit() || matches!(c, ' ' | '+' | '-' | '/' | '(' | ')' | '.')
            });
        let digits: String = query
            .chars()
            .filter(char::is_ascii_digit)
            .collect::<String>()
            .trim_start_matches('0')
            .to_owned();
        if is_phone && digits.len() >= 3 && words.len() > 1 {
            let phone: Box<dyn Query> = Box::new(BoostQuery::new(
                Box::new(FuzzyTermQuery::new_prefix(
                    Term::from_field_text(self.fields.phones, &digits),
                    0,
                    true,
                )),
                1.5,
            ));
            return Some(Box::new(BooleanQuery::new(vec![
                (Occur::Should, words_query),
                (Occur::Should, phone),
            ])));
        }
        if is_phone && digits.len() >= 3 && words.len() == 1 && words[0] != digits {
            // "0301234": the leading zero is not stored.
            let phone: Box<dyn Query> = Box::new(FuzzyTermQuery::new_prefix(
                Term::from_field_text(self.fields.phones, &digits),
                0,
                true,
            ));
            return Some(Box::new(BooleanQuery::new(vec![
                (Occur::Should, words_query),
                (Occur::Should, phone),
            ])));
        }
        Some(words_query)
    }

    fn any_term(field: Field, values: &[&str]) -> Box<dyn Query> {
        Box::new(BooleanQuery::new(
            values
                .iter()
                .map(|v| {
                    let term: Box<dyn Query> = Box::new(TermQuery::new(
                        Term::from_field_text(field, v),
                        IndexRecordOption::Basic,
                    ));
                    (Occur::Should, term)
                })
                .collect(),
        ))
    }

    /// Items matching `query` (see the module docs), best first: of the
    /// given kinds (all when empty; contacts include groups) and, when
    /// `collections` is given, in those calendars / address books.
    pub fn search(
        &self,
        query: &str,
        kinds: &[PimItemKind],
        collections: Option<&[&str]>,
        limit: usize,
    ) -> Result<Vec<(PimItemRef, f32)>> {
        let Some(text) = self.text_query(query) else {
            return Ok(Vec::new());
        };
        if limit == 0 || collections.is_some_and(<[&str]>::is_empty) {
            return Ok(Vec::new());
        }
        let mut clauses: Vec<(Occur, Box<dyn Query>)> = vec![(Occur::Must, text)];
        if !kinds.is_empty() {
            let mut names: Vec<&str> = kinds.iter().map(|k| kind_str(*k)).collect();
            if kinds.contains(&PimItemKind::Contact) {
                names.push(kind_str(PimItemKind::ContactGroup));
            }
            clauses.push((Occur::Must, Self::any_term(self.fields.kind, &names)));
        }
        if let Some(collections) = collections {
            clauses.push((
                Occur::Must,
                Self::any_term(self.fields.collection, collections),
            ));
        }
        self.commit_pending()?;
        let searcher = self.reader.searcher();
        let hits = searcher.search(
            &BooleanQuery::new(clauses),
            &TopDocs::with_limit(limit).order_by_score(),
        )?;
        let mut out = Vec::with_capacity(hits.len());
        for (score, address) in hits {
            let doc: TantivyDocument = searcher.doc(address)?;
            if let Some(item) = doc
                .get_first(self.fields.key)
                .and_then(|v| v.as_str())
                .and_then(item_of_key)
            {
                out.push((item, score));
            }
        }
        Ok(out)
    }

    /// Contacts (not groups) matching `query`, best first, in the given
    /// address books (all when `None`).
    pub fn search_contacts(
        &self,
        query: &str,
        books: Option<&[AddressBookId]>,
        limit: usize,
    ) -> Result<Vec<(ContactId, f32)>> {
        let books: Option<Vec<&str>> = books.map(|b| b.iter().map(AddressBookId::as_str).collect());
        // Groups are filtered out after the search; ask for a few more.
        let hits = self.search(query, &[PimItemKind::Contact], books.as_deref(), limit + 20)?;
        Ok(hits
            .into_iter()
            .filter_map(|(item, score)| match item {
                PimItemRef::Contact { contact_id } => Some((contact_id, score)),
                _ => None,
            })
            .take(limit)
            .collect())
    }
}

/// Which field of a contact a query matched, with the matching value
/// (for `ContactMatch`): the name first, then an address, a phone
/// number, the organization; `Other` otherwise.
pub fn contact_match(contact: &Contact, query: &str) -> (ContactMatchField, Option<String>) {
    let words = tokens(query);
    let matches = |text: &str| {
        let have = tokens(text);
        !words.is_empty()
            && words
                .iter()
                .all(|w| have.iter().any(|h| h.starts_with(w.as_str())))
    };
    let f = &contact.fields;
    let mut name_parts = vec![f.display_name.clone()];
    if let Some(n) = &f.name {
        for part in [&n.prefix, &n.given, &n.additional, &n.family, &n.suffix] {
            name_parts.extend(part.clone());
        }
    }
    name_parts.extend(f.nicknames.iter().cloned());
    if matches(&name_parts.join(" ")) {
        return (ContactMatchField::Name, None);
    }
    if let Some(email) = f.emails.iter().find(|e| matches(&e.value)) {
        return (ContactMatchField::Email, Some(email.value.clone()));
    }
    let digits: String = query
        .chars()
        .filter(char::is_ascii_digit)
        .collect::<String>()
        .trim_start_matches('0')
        .to_owned();
    if let Some(phone) = f.phones.iter().find(|p| {
        matches(&p.value)
            || (digits.len() >= 3
                && phone_digit_forms(&p.value)
                    .iter()
                    .any(|d| d.starts_with(&digits)))
    }) {
        return (ContactMatchField::Phone, Some(phone.value.clone()));
    }
    if let Some(org) = f.organization.as_ref().filter(|o| matches(&o.name)) {
        return (ContactMatchField::Organization, Some(org.name.clone()));
    }
    (ContactMatchField::Other, None)
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;

    use nail_api::pim::contact::{ContactName, ContactOrg, VCardVersion};
    use nail_api::pim::{
        Attendee, ContactFields, LabeledValue, PimItemSync, PimTime, PostalAddress, TaskFields,
    };
    use pretty_assertions::assert_eq;

    use super::*;

    fn index() -> PimIndex {
        PimIndex::open_in_ram(CommitPolicy::default()).unwrap()
    }

    fn event(id: &str, cal: &str, title: &str) -> CalendarEvent {
        CalendarEvent {
            id: id.into(),
            calendar_id: cal.into(),
            uid: format!("u-{id}"),
            fields: EventFields::new(
                title,
                PimTime::utc("2026-10-01T09:00:00"),
                PimTime::utc("2026-10-01T10:00:00"),
            ),
            recurrence: None,
            overrides: Vec::new(),
            sequence: 0,
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    fn contact(id: &str, book: &str, name: &str) -> Contact {
        Contact {
            id: id.into(),
            address_book_id: book.into(),
            uid: format!("u-{id}"),
            fields: ContactFields::named(name),
            version: VCardVersion::V4,
            group_ids: Vec::new(),
            linked_ids: Vec::new(),
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    fn task(id: &str, title: &str) -> Task {
        Task {
            id: id.into(),
            calendar_id: "tasks".into(),
            uid: format!("u-{id}"),
            fields: TaskFields::titled(title),
            recurrence: None,
            subtask_ids: Vec::new(),
            created_at: None,
            updated_at: None,
            read_only: false,
            sync: PimItemSync::Synced,
        }
    }

    fn keys(hits: &[(PimItemRef, f32)]) -> Vec<String> {
        hits.iter().map(|(i, _)| item_key(i)).collect()
    }

    fn ada() -> Contact {
        let mut ada = contact("c1", "book", "Ada Lovelace");
        ada.fields.name = Some(ContactName {
            given: Some("Ada".into()),
            family: Some("Lovelace".into()),
            ..ContactName::default()
        });
        ada.fields.emails = vec![LabeledValue::new("ada@analytical.example")];
        ada.fields.phones = vec![LabeledValue::new("+49 30 1234567")];
        ada.fields.organization = Some(ContactOrg {
            name: "Analytical Engines".into(),
            units: Vec::new(),
        });
        ada.fields.addresses = vec![PostalAddress {
            locality: Some("München".into()),
            ..PostalAddress::default()
        }];
        ada.fields.notes = Some("Met at the Babbage lecture".into());
        ada
    }

    #[test]
    fn finds_events_tasks_and_contacts_as_you_type() {
        let index = index();
        let mut review = event("e1", "work", "Quarterly review");
        review.fields.location = Some("Room Lovelace".into());
        review.fields.attendees = vec![Attendee {
            name: Some("Bob Builder".into()),
            ..Attendee::new("bob@example.org")
        }];
        index
            .add(&[
                PimDocument::event(&review),
                PimDocument::event(&event("e2", "home", "Dentist")),
                PimDocument::task(&task("t1", "Prepare quarterly numbers")),
                PimDocument::contact(&ada()),
            ])
            .unwrap();
        assert_eq!(index.doc_count().unwrap(), 4);
        // Word starts, any kind; titles rank above other fields.
        let hits = index.search("quart", &[], None, 10).unwrap();
        assert_eq!(keys(&hits), ["event:e1", "task:t1"]);
        // "lovelace": the contact's name beats the event's location.
        let hits = index.search("lovelace", &[], None, 10).unwrap();
        assert_eq!(keys(&hits), ["contact:c1", "event:e1"]);
        // Every word must match.
        assert_eq!(
            keys(&index.search("ad lovel", &[], None, 10).unwrap()),
            ["contact:c1"]
        );
        assert!(
            index
                .search("ada dentist", &[], None, 10)
                .unwrap()
                .is_empty(),
            "{:?}",
            index.search("ada dentist", &[], None, 10).unwrap()
        );
        // Attendees, addresses (folded), notes, organizations.
        assert_eq!(
            keys(&index.search("builder", &[], None, 10).unwrap()),
            ["event:e1"]
        );
        assert_eq!(
            keys(&index.search("munchen", &[], None, 10).unwrap()),
            ["contact:c1"]
        );
        assert_eq!(
            keys(&index.search("babbage", &[], None, 10).unwrap()),
            ["contact:c1"]
        );
        assert_eq!(
            keys(&index.search("analytical", &[], None, 10).unwrap()),
            ["contact:c1"]
        );
        // Kinds and collections.
        assert_eq!(
            keys(
                &index
                    .search("quart", &[PimItemKind::Task], None, 10)
                    .unwrap()
            ),
            ["task:t1"]
        );
        assert_eq!(
            keys(&index.search("quart", &[], Some(&["work"]), 10).unwrap()),
            ["event:e1"]
        );
        assert!(
            index
                .search("quart", &[], Some(&[]), 10)
                .unwrap()
                .is_empty(),
            "{:?}",
            index.search("quart", &[], Some(&[]), 10).unwrap()
        );
        assert_eq!(index.search("  ", &[], None, 10).unwrap().len(), 0);
        assert_eq!(index.search("quart", &[], None, 1).unwrap().len(), 1);
    }

    #[test]
    fn phone_numbers_by_digits() {
        let index = index();
        index.add(&[PimDocument::contact(&ada())]).unwrap();
        for query in ["030 1234", "0301234567", "+49 30 123", "1234567", "301234"] {
            assert_eq!(
                keys(&index.search(query, &[], None, 10).unwrap()),
                ["contact:c1"],
                "{query}"
            );
        }
        assert_eq!(index.search("030 999", &[], None, 10).unwrap().len(), 0);
    }

    #[test]
    fn replace_remove_and_collections() {
        let index = index();
        index
            .add(&[
                PimDocument::event(&event("e1", "work", "Standup")),
                PimDocument::event(&event("e2", "work", "Retro")),
                PimDocument::event(&event("e3", "home", "Standup at home")),
            ])
            .unwrap();
        index
            .add(&[PimDocument::event(&event("e1", "work", "Daily"))])
            .unwrap();
        assert_eq!(
            keys(&index.search("standup", &[], None, 10).unwrap()),
            ["event:e3"]
        );
        assert_eq!(
            keys(&index.search("daily", &[], None, 10).unwrap()),
            ["event:e1"]
        );
        index
            .remove(&[PimItemRef::Event {
                event_id: "e1".into(),
            }])
            .unwrap();
        assert_eq!(index.search("daily", &[], None, 10).unwrap().len(), 0);
        index.remove_collection("work").unwrap();
        assert_eq!(index.doc_count().unwrap(), 1);
        index.clear().unwrap();
        assert_eq!(index.doc_count().unwrap(), 0);
    }

    #[test]
    fn contacts_only_and_groups() {
        let index = index();
        let mut team = ContactGroup {
            id: "g1".into(),
            address_book_id: "book".into(),
            uid: "ug".into(),
            name: "Analysts".into(),
            member_ids: Vec::new(),
            member_emails: vec!["eve@example.org".into()],
            read_only: false,
            sync: PimItemSync::Synced,
        };
        index
            .add(&[
                PimDocument::contact(&ada()),
                PimDocument::contact(&contact("c2", "other", "Ana Analyse")),
                PimDocument::group(&team),
            ])
            .unwrap();
        let hits = index
            .search("analy", &[PimItemKind::Contact], None, 10)
            .unwrap();
        assert_eq!(hits.len(), 3, "groups count as contacts");
        let contacts = index.search_contacts("analy", None, 10).unwrap();
        assert_eq!(contacts.len(), 2);
        let books = [AddressBookId::new("other")];
        let contacts = index.search_contacts("ana", Some(&books), 10).unwrap();
        assert_eq!(contacts, vec![(ContactId::new("c2"), contacts[0].1)]);
        team.name = "Renamed".into();
        index.add(&[PimDocument::group(&team)]).unwrap();
        assert_eq!(
            keys(&index.search("eve", &[], None, 10).unwrap()),
            ["group:g1"]
        );
    }

    #[test]
    fn match_fields() {
        let ada = ada();
        assert_eq!(
            contact_match(&ada, "ada lovel"),
            (ContactMatchField::Name, None)
        );
        assert_eq!(
            contact_match(&ada, "analytical.ex"),
            (
                ContactMatchField::Email,
                Some("ada@analytical.example".into())
            )
        );
        assert_eq!(
            contact_match(&ada, "030 123"),
            (ContactMatchField::Phone, Some("+49 30 1234567".into()))
        );
        assert_eq!(
            contact_match(&ada, "engines"),
            (
                ContactMatchField::Organization,
                Some("Analytical Engines".into())
            )
        );
        assert_eq!(
            contact_match(&ada, "babbage"),
            (ContactMatchField::Other, None)
        );
    }

    #[test]
    fn overrides_are_searchable() {
        let index = index();
        let mut series = event("s", "work", "Weekly sync");
        let mut moved = series.fields.clone();
        moved.title = "Weekly sync (offsite)".into();
        moved.location = Some("Lighthouse".into());
        series.overrides = vec![nail_api::pim::EventOverride {
            recurrence_id: PimTime::utc("2026-10-08T09:00:00"),
            fields: moved,
            this_and_future: false,
        }];
        index.add(&[PimDocument::event(&series)]).unwrap();
        assert_eq!(
            keys(&index.search("lighthouse", &[], None, 10).unwrap()),
            ["event:s"]
        );
        assert_eq!(
            keys(&index.search("offsite", &[], None, 10).unwrap()),
            ["event:s"]
        );
    }

    #[test]
    fn on_disk_with_a_wrapper_and_rebuilds() {
        use std::sync::atomic::{AtomicUsize, Ordering};
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join(PIM_INDEX_DIR_NAME);
        let calls = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&calls);
        let wrapper: DirectoryWrapper = Arc::new(move |inner| {
            counter.fetch_add(1, Ordering::SeqCst);
            inner
        });
        {
            let index =
                PimIndex::open_with(&path, CommitPolicy::default(), Some(&wrapper)).unwrap();
            assert!(!index.was_rebuilt());
            index.add(&[PimDocument::contact(&ada())]).unwrap();
            index.commit().unwrap();
        }
        assert_eq!(calls.load(Ordering::SeqCst), 1);
        {
            let index =
                PimIndex::open_with(&path, CommitPolicy::default(), Some(&wrapper)).unwrap();
            assert!(!index.was_rebuilt());
            assert_eq!(index.doc_count().unwrap(), 1);
        }
        // Another schema version or a damaged index is rebuilt empty.
        std::fs::write(path.join(VERSION_FILE_NAME), "0").unwrap();
        let index = PimIndex::open(&path, CommitPolicy::default()).unwrap();
        assert!(index.was_rebuilt());
        assert_eq!(index.doc_count().unwrap(), 0);
        drop(index);
        std::fs::write(path.join("meta.json"), "not json").unwrap();
        let index = PimIndex::open(&path, CommitPolicy::default()).unwrap();
        assert!(index.was_rebuilt());
    }

    #[test]
    fn commit_batching() {
        let policy = CommitPolicy {
            max_pending: 2,
            max_delay: std::time::Duration::from_hours(1),
        };
        let index = PimIndex::open_in_ram(policy).unwrap();
        index.add(&[PimDocument::task(&task("t1", "a"))]).unwrap();
        assert!(!index.commit_if_due().unwrap());
        index.add(&[PimDocument::task(&task("t2", "b"))]).unwrap();
        // Committed at max_pending; searches see everything anyway.
        assert!(!index.commit_if_due().unwrap());
        assert_eq!(index.doc_count().unwrap(), 2);
        assert_eq!(PIM_KINDS.len(), 3);
    }

    #[test]
    fn keys_round_trip() {
        for item in [
            PimItemRef::Event {
                event_id: "a:b".into(),
            },
            PimItemRef::Task {
                task_id: "t".into(),
            },
            PimItemRef::Contact {
                contact_id: "c".into(),
            },
            PimItemRef::ContactGroup {
                group_id: "g".into(),
            },
        ] {
            assert_eq!(item_of_key(&item_key(&item)), Some(item));
        }
        assert_eq!(item_of_key("other:x"), None);
        assert_eq!(
            phone_digit_forms("+49 (0)30 1234567"),
            ["490301234567", "301234567"]
        );
        assert_eq!(phone_digit_forms("n/a").len(), 0);
    }
}
