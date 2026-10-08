//! The in-memory Gmail mailbox behind [`super::MockGmailServer`]: messages
//! with labels and threads, user labels, drafts, the history log (with
//! expiry), Gmail search for the terms Nail uses, and the JSON views the
//! API answers with.

use std::collections::{BTreeMap, BTreeSet};

use mail_parser::MimeHeaders as _;
use nail_api::gmail::{GmailCategory, system};
use serde::Deserialize;
use serde_json::{Value, json};

use crate::labels::is_palette_color;
use crate::wire::{
    ErrorBody, Header, History, HistoryLabels, HistoryMessage, Label, LabelColor, Message,
    MessagePart, MessagePartBody, decode_base64url, encode_base64url,
};

/// An error answer of the mock: status and Gmail's error body.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockError {
    /// HTTP status.
    pub status: u16,
    /// Body.
    pub body: ErrorBody,
}

impl MockError {
    pub(crate) fn not_found() -> Self {
        Self {
            status: 404,
            body: ErrorBody::new(
                404,
                "NOT_FOUND",
                "notFound",
                "Requested entity was not found.",
            ),
        }
    }

    pub(crate) fn bad_request(message: &str) -> Self {
        Self {
            status: 400,
            body: ErrorBody::new(400, "INVALID_ARGUMENT", "invalidArgument", message),
        }
    }

    pub(crate) fn conflict(message: &str) -> Self {
        Self {
            status: 409,
            body: ErrorBody::new(409, "ALREADY_EXISTS", "alreadyExists", message),
        }
    }
}

/// A search predicate.
type Test<'a> = Box<dyn Fn(&MockMessage) -> bool + 'a>;

/// Mock result.
pub(super) type MockResult<T> = Result<T, MockError>;

/// A stored message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockMessage {
    /// Gmail id.
    pub id: String,
    /// Thread id.
    pub thread_id: String,
    /// Labels.
    pub label_ids: Vec<String>,
    /// Raw RFC 5322 bytes.
    pub raw: Vec<u8>,
    /// Receive time, ms since the epoch.
    pub internal_date_ms: i64,
    /// Last history id that touched it.
    pub history_id: u64,
}

impl MockMessage {
    fn has(&self, label: &str) -> bool {
        self.label_ids.iter().any(|l| l == label)
    }
}

/// The system labels Gmail lists (`labels.list`), with their visibility.
const SYSTEM_LABELS: [(&str, Option<&str>, Option<&str>); 14] = [
    ("CHAT", Some("hide"), Some("labelHide")),
    ("SENT", None, None),
    ("INBOX", None, None),
    ("IMPORTANT", Some("hide"), Some("labelHide")),
    ("TRASH", Some("hide"), Some("labelHide")),
    ("DRAFT", None, None),
    ("SPAM", Some("hide"), Some("labelHide")),
    ("CATEGORY_FORUMS", None, None),
    ("CATEGORY_UPDATES", None, None),
    ("CATEGORY_PERSONAL", None, None),
    ("CATEGORY_PROMOTIONS", None, None),
    ("CATEGORY_SOCIAL", None, None),
    ("STARRED", None, None),
    ("UNREAD", None, None),
];

/// The header lines of a raw message (unfolded, lossy UTF-8).
pub(crate) fn raw_headers(raw: &[u8]) -> Vec<(String, String)> {
    let text = String::from_utf8_lossy(raw);
    let head = text
        .split("\r\n\r\n")
        .next()
        .unwrap_or_default()
        .split("\n\n")
        .next()
        .unwrap_or_default()
        .to_owned();
    let mut out: Vec<(String, String)> = Vec::new();
    for line in head.lines() {
        let line = line.trim_end_matches('\r');
        if line.starts_with([' ', '\t']) {
            if let Some(last) = out.last_mut() {
                last.1.push(' ');
                last.1.push_str(line.trim());
            }
        } else if let Some((name, value)) = line.split_once(':') {
            out.push((name.trim().to_owned(), value.trim().to_owned()));
        }
    }
    out
}

use nail_util::named::first as header_value;

fn strip_id(id: &str) -> String {
    id.trim()
        .trim_start_matches('<')
        .trim_end_matches('>')
        .to_owned()
}

/// The mailbox.
#[derive(Debug, Clone)]
pub struct Mailbox {
    /// The address (`users.getProfile`).
    pub email: String,
    history_id: u64,
    oldest_history: u64,
    next_id: u64,
    next_label: u64,
    messages: BTreeMap<String, MockMessage>,
    labels: BTreeMap<String, Label>,
    history: Vec<History>,
    drafts: BTreeMap<String, String>,
    sent: Vec<Vec<u8>>,
}

/// A mailbox fixture (`fixtures/mailbox-*.json`): the profile, user labels
/// as `labels.list` returns them and messages as `messages.get?format=raw`
/// returns them (`raw` base64url, or `rawText` for readability).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Fixture {
    /// Address.
    pub email_address: String,
    /// Start history id.
    #[serde(default)]
    pub history_id: Option<String>,
    /// Labels (system labels are added when missing).
    #[serde(default)]
    pub labels: Vec<Label>,
    /// Messages.
    #[serde(default)]
    pub messages: Vec<FixtureMessage>,
}

/// A message of a [`Fixture`].
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FixtureMessage {
    /// Gmail id.
    pub id: String,
    /// Thread id (default: the id).
    #[serde(default)]
    pub thread_id: Option<String>,
    /// Labels.
    #[serde(default)]
    pub label_ids: Vec<String>,
    /// Receive time, ms (default: the `Date` header).
    #[serde(default)]
    pub internal_date: Option<String>,
    /// The message as base64url.
    #[serde(default)]
    pub raw: Option<String>,
    /// The message as text (LF line endings become CRLF).
    #[serde(default)]
    pub raw_text: Option<String>,
}

impl Mailbox {
    /// An empty mailbox with Gmail's system labels at history id 1000.
    pub fn new(email: &str) -> Self {
        let mut labels = BTreeMap::new();
        for (id, message_visibility, list_visibility) in SYSTEM_LABELS {
            labels.insert(
                id.to_owned(),
                Label {
                    id: id.to_owned(),
                    name: id.to_owned(),
                    kind: Some("system".to_owned()),
                    message_list_visibility: message_visibility.map(str::to_owned),
                    label_list_visibility: list_visibility.map(str::to_owned),
                    ..Label::default()
                },
            );
        }
        Self {
            email: email.to_owned(),
            history_id: 1000,
            oldest_history: 0,
            next_id: 1,
            next_label: 1,
            messages: BTreeMap::new(),
            labels,
            history: Vec::new(),
            drafts: BTreeMap::new(),
            sent: Vec::new(),
        }
    }

    /// A mailbox from a fixture (JSON text).
    pub fn from_fixture(json: &str) -> Result<Self, String> {
        let fixture: Fixture = serde_json::from_str(json).map_err(|e| e.to_string())?;
        let mut mailbox = Self::new(&fixture.email_address);
        if let Some(h) = &fixture.history_id {
            mailbox.history_id = h.parse().map_err(|_| format!("bad historyId {h}"))?;
        }
        for label in fixture.labels {
            if let Some(n) = label
                .id
                .strip_prefix("Label_")
                .and_then(|n| n.parse::<u64>().ok())
            {
                mailbox.next_label = mailbox.next_label.max(n + 1);
            }
            mailbox.labels.insert(label.id.clone(), label);
        }
        for m in fixture.messages {
            let raw = match (&m.raw, &m.raw_text) {
                (Some(raw), _) => decode_base64url(raw).map_err(|e| e.to_string())?,
                (None, Some(text)) => text
                    .replace("\r\n", "\n")
                    .replace('\n', "\r\n")
                    .into_bytes(),
                (None, None) => return Err(format!("message {} has no raw", m.id)),
            };
            for label in &m.label_ids {
                if !mailbox.labels.contains_key(label) {
                    return Err(format!("message {} has unknown label {label}", m.id));
                }
            }
            let internal_date_ms = match &m.internal_date {
                Some(d) => d.parse().map_err(|_| format!("bad internalDate {d}"))?,
                None => date_of(&raw),
            };
            mailbox.messages.insert(
                m.id.clone(),
                MockMessage {
                    thread_id: m.thread_id.clone().unwrap_or_else(|| m.id.clone()),
                    id: m.id,
                    label_ids: m.label_ids,
                    raw,
                    internal_date_ms,
                    history_id: mailbox.history_id,
                },
            );
        }
        Ok(mailbox)
    }

    /// The current history id.
    pub fn history_id(&self) -> u64 {
        self.history_id
    }

    /// Forgets all history so far: a `startHistoryId` before now answers
    /// `404` (Gmail keeps about a week).
    pub fn expire_history(&mut self) {
        self.oldest_history = self.history_id;
        self.history.clear();
    }

    /// A stored message.
    pub fn message(&self, id: &str) -> Option<&MockMessage> {
        self.messages.get(id)
    }

    /// Every message id.
    pub fn message_ids(&self) -> Vec<String> {
        self.messages.keys().cloned().collect()
    }

    /// Every label.
    pub fn labels(&self) -> Vec<Label> {
        self.labels.values().cloned().collect()
    }

    /// Messages sent through `messages.send`, as Gmail received them.
    pub fn sent(&self) -> &[Vec<u8>] {
        &self.sent
    }

    fn record(&mut self, fill: impl FnOnce(&mut History)) -> u64 {
        self.history_id += 1;
        let mut record = History {
            id: self.history_id.to_string(),
            ..History::default()
        };
        fill(&mut record);
        self.history.push(record);
        self.history_id
    }

    fn history_message(m: &MockMessage) -> Message {
        Message {
            id: m.id.clone(),
            thread_id: m.thread_id.clone(),
            label_ids: m.label_ids.clone(),
            ..Message::default()
        }
    }

    fn new_id(&mut self) -> String {
        let id = format!("{:016x}", 0x18f0_0000_0000_0000_u64 + self.next_id);
        self.next_id += 1;
        id
    }

    fn check_labels(&self, labels: &[String]) -> MockResult<()> {
        for label in labels {
            if !self.labels.contains_key(label) {
                return Err(MockError::bad_request(&format!("Invalid label: {label}")));
            }
        }
        Ok(())
    }

    /// The thread a new message joins: `thread_id` if known, else the
    /// thread of a message it replies to, else its own.
    fn thread_for(&self, raw: &[u8], thread_id: Option<&str>, own: &str) -> String {
        if let Some(t) = thread_id
            && self.messages.values().any(|m| m.thread_id == t)
        {
            return t.to_owned();
        }
        let headers = raw_headers(raw);
        let parents: Vec<String> = ["In-Reply-To", "References"]
            .iter()
            .filter_map(|h| header_value(&headers, h))
            .flat_map(|v| v.split_whitespace().map(strip_id).collect::<Vec<_>>())
            .collect();
        for parent in parents {
            if let Some(m) = self
                .messages
                .values()
                .find(|m| message_id_of(&m.raw) == Some(parent.clone()))
            {
                return m.thread_id.clone();
            }
        }
        own.to_owned()
    }

    /// Adds a message (delivery, `insert`, `send`, `drafts.create`).
    pub fn add_message(
        &mut self,
        raw: Vec<u8>,
        labels: &[String],
        thread_id: Option<&str>,
    ) -> MockResult<String> {
        self.check_labels(labels)?;
        if raw.is_empty() {
            return Err(MockError::bad_request(
                "'raw' RFC822 payload message string or uploading message via /upload/* URL required",
            ));
        }
        let id = self.new_id();
        let thread_id = self.thread_for(&raw, thread_id, &id);
        let mut label_ids: Vec<String> = Vec::new();
        for l in labels {
            if !label_ids.contains(l) {
                label_ids.push(l.clone());
            }
        }
        let message = MockMessage {
            id: id.clone(),
            thread_id,
            label_ids,
            internal_date_ms: date_of(&raw),
            raw,
            history_id: 0,
        };
        let entry = Self::history_message(&message);
        self.messages.insert(id.clone(), message);
        let h = self.record(|r| {
            r.messages = vec![entry.clone()];
            r.messages_added = vec![HistoryMessage { message: entry }];
        });
        if let Some(m) = self.messages.get_mut(&id) {
            m.history_id = h;
        }
        Ok(id)
    }

    /// Adds and removes labels (`modify`); records only real changes.
    pub fn modify(
        &mut self,
        id: &str,
        add: &[String],
        remove: &[String],
    ) -> MockResult<&MockMessage> {
        self.check_labels(add)?;
        self.check_labels(remove)?;
        let message = self.messages.get_mut(id).ok_or_else(MockError::not_found)?;
        let mut added = Vec::new();
        let mut removed = Vec::new();
        for l in remove {
            if message.label_ids.contains(l) && !add.contains(l) {
                message.label_ids.retain(|x| x != l);
                removed.push(l.clone());
            }
        }
        for l in add {
            if !message.label_ids.contains(l) {
                message.label_ids.push(l.clone());
                added.push(l.clone());
            }
        }
        if !added.is_empty() || !removed.is_empty() {
            let entry = Self::history_message(message);
            let h = self.record(|r| {
                r.messages = vec![entry.clone()];
                if !added.is_empty() {
                    r.labels_added = vec![HistoryLabels {
                        message: entry.clone(),
                        label_ids: added,
                    }];
                }
                if !removed.is_empty() {
                    r.labels_removed = vec![HistoryLabels {
                        message: entry,
                        label_ids: removed,
                    }];
                }
            });
            if let Some(m) = self.messages.get_mut(id) {
                m.history_id = h;
            }
        }
        self.messages.get(id).ok_or_else(MockError::not_found)
    }

    /// `messages.trash`: adds `TRASH` and drops `SPAM`; the other labels
    /// stay (restoring brings the message back where it was).
    pub fn trash(&mut self, id: &str) -> MockResult<&MockMessage> {
        self.modify(id, &[system::TRASH.to_owned()], &[system::SPAM.to_owned()])
    }

    /// `messages.untrash`.
    pub fn untrash(&mut self, id: &str) -> MockResult<&MockMessage> {
        self.modify(id, &[], &[system::TRASH.to_owned()])
    }

    /// `messages.delete`: removes the message for good.
    pub fn delete(&mut self, id: &str) -> MockResult<()> {
        let message = self.messages.remove(id).ok_or_else(MockError::not_found)?;
        self.drafts.retain(|_, m| m != id);
        let entry = Message {
            id: message.id.clone(),
            thread_id: message.thread_id.clone(),
            ..Message::default()
        };
        self.record(|r| {
            r.messages = vec![entry.clone()];
            r.messages_deleted = vec![HistoryMessage { message: entry }];
        });
        Ok(())
    }

    /// `messages.send`: stores the message in Sent. Gmail needs a
    /// recipient header.
    pub fn send(&mut self, raw: Vec<u8>, thread_id: Option<&str>) -> MockResult<String> {
        let headers = raw_headers(&raw);
        if !["To", "Cc", "Bcc"]
            .iter()
            .any(|h| header_value(&headers, h).is_some_and(|v| v.contains('@')))
        {
            return Err(MockError::bad_request("Recipient address required"));
        }
        // Gmail removes Bcc from the stored copy.
        let stored = strip_header(&raw, "Bcc");
        self.sent.push(raw);
        self.add_message(stored, &[system::SENT.to_owned()], thread_id)
    }

    /// `drafts.create`.
    pub fn create_draft(
        &mut self,
        raw: Vec<u8>,
        thread_id: Option<&str>,
    ) -> MockResult<(String, String)> {
        let id = self.add_message(raw, &[system::DRAFT.to_owned()], thread_id)?;
        let draft = format!("r{}", self.drafts.len() + 1);
        self.drafts.insert(draft.clone(), id.clone());
        Ok((draft, id))
    }

    // -- labels ---------------------------------------------------------

    fn validate_label(&self, label: &Label, except: Option<&str>) -> MockResult<()> {
        let name = label.name.trim();
        if name.is_empty() {
            return Err(MockError::bad_request("Invalid label name"));
        }
        if self
            .labels
            .values()
            .any(|l| Some(l.id.as_str()) != except && l.name.eq_ignore_ascii_case(name))
        {
            return Err(MockError::conflict("Label name exists or conflicts"));
        }
        if let Some(color) = &label.color {
            for c in [&color.background_color, &color.text_color] {
                if !is_palette_color(c) {
                    return Err(MockError::bad_request(&format!(
                        "Label color {c} is not on the allowed color palette"
                    )));
                }
            }
        }
        Ok(())
    }

    /// `labels.create`.
    pub fn create_label(&mut self, label: Label) -> MockResult<Label> {
        self.validate_label(&label, None)?;
        let id = format!("Label_{}", self.next_label);
        self.next_label += 1;
        let stored = Label {
            id: id.clone(),
            name: label.name.trim().to_owned(),
            kind: Some("user".to_owned()),
            message_list_visibility: label.message_list_visibility.or(Some("show".into())),
            label_list_visibility: label.label_list_visibility.or(Some("labelShow".into())),
            color: label.color,
            ..Label::default()
        };
        self.labels.insert(id, stored.clone());
        Ok(stored)
    }

    /// `labels.patch` (`replace` = `labels.update`).
    pub fn update_label(&mut self, id: &str, patch: &Value, replace: bool) -> MockResult<Label> {
        let current = self
            .labels
            .get(id)
            .ok_or_else(MockError::not_found)?
            .clone();
        if current.kind.as_deref() == Some("system") {
            return Err(MockError::bad_request(
                "Invalid label: system labels cannot be changed",
            ));
        }
        let mut next = if replace {
            Label {
                id: id.to_owned(),
                kind: current.kind.clone(),
                ..Label::default()
            }
        } else {
            current.clone()
        };
        if let Some(name) = patch.get("name").and_then(Value::as_str) {
            name.trim().clone_into(&mut next.name);
        }
        for (key, slot) in [
            ("labelListVisibility", &mut next.label_list_visibility),
            ("messageListVisibility", &mut next.message_list_visibility),
        ] {
            if let Some(v) = patch.get(key).and_then(Value::as_str) {
                *slot = Some(v.to_owned());
            }
        }
        match patch.get("color") {
            Some(Value::Null) => next.color = None,
            Some(c) => {
                next.color = Some(
                    serde_json::from_value::<LabelColor>(c.clone())
                        .map_err(|_| MockError::bad_request("Invalid color"))?,
                );
            }
            None => {}
        }
        self.validate_label(&next, Some(id))?;
        self.labels.insert(id.to_owned(), next.clone());
        Ok(next)
    }

    /// `labels.delete`: also removes it from every message (recorded).
    pub fn delete_label(&mut self, id: &str) -> MockResult<()> {
        let label = self.labels.get(id).ok_or_else(MockError::not_found)?;
        if label.kind.as_deref() == Some("system") {
            return Err(MockError::bad_request(
                "Invalid label: system labels cannot be deleted",
            ));
        }
        let ids: Vec<String> = self
            .messages
            .values()
            .filter(|m| m.has(id))
            .map(|m| m.id.clone())
            .collect();
        for m in ids {
            self.modify(&m, &[], &[id.to_owned()])?;
        }
        self.labels.remove(id);
        Ok(())
    }

    /// A label with its counts (`labels.get`).
    pub fn label_with_counts(&self, id: &str) -> MockResult<Label> {
        let mut label = self
            .labels
            .get(id)
            .ok_or_else(MockError::not_found)?
            .clone();
        let with: Vec<&MockMessage> = self.messages.values().filter(|m| m.has(id)).collect();
        let unread: Vec<&&MockMessage> = with.iter().filter(|m| m.has(system::UNREAD)).collect();
        let threads: BTreeSet<&str> = with.iter().map(|m| m.thread_id.as_str()).collect();
        let unread_threads: BTreeSet<&str> = unread.iter().map(|m| m.thread_id.as_str()).collect();
        label.messages_total = Some(with.len() as u64);
        label.messages_unread = Some(unread.len() as u64);
        label.threads_total = Some(threads.len() as u64);
        label.threads_unread = Some(unread_threads.len() as u64);
        Ok(label)
    }

    /// The label id for a `label:` search term.
    fn label_by_name(&self, name: &str) -> Option<String> {
        let wanted = name.replace('-', " ");
        self.labels
            .values()
            .find(|l| {
                l.id.eq_ignore_ascii_case(name)
                    || l.name.eq_ignore_ascii_case(name)
                    || l.name.eq_ignore_ascii_case(&wanted)
            })
            .map(|l| l.id.clone())
    }

    // -- search ---------------------------------------------------------

    /// Message ids matching a list request, newest first.
    pub fn search(
        &self,
        q: Option<&str>,
        label_ids: &[String],
        include_spam_trash: bool,
    ) -> MockResult<Vec<&MockMessage>> {
        let mut anywhere = include_spam_trash
            || label_ids
                .iter()
                .any(|l| l == system::SPAM || l == system::TRASH);
        let mut tests: Vec<Test<'_>> = Vec::new();
        for term in q.unwrap_or_default().split_whitespace() {
            let (key, value) = term.split_once(':').unwrap_or((term, ""));
            let value = value.to_owned();
            match key.to_ascii_lowercase().as_str() {
                "after" | "before" => {
                    let secs = parse_search_date(&value)
                        .ok_or_else(|| MockError::bad_request(&format!("Invalid query: {term}")))?;
                    let after = key.eq_ignore_ascii_case("after");
                    tests.push(Box::new(move |m| {
                        if after {
                            m.internal_date_ms / 1000 > secs
                        } else {
                            m.internal_date_ms / 1000 < secs
                        }
                    }));
                }
                "rfc822msgid" => {
                    let wanted = strip_id(&value);
                    anywhere = true;
                    tests.push(Box::new(move |m| {
                        message_id_of(&m.raw).as_deref() == Some(wanted.as_str())
                    }));
                }
                "in" if value == "anywhere" => anywhere = true,
                "in" | "label" => {
                    let id = self
                        .label_by_name(&value)
                        .ok_or_else(|| MockError::bad_request(&format!("Invalid query: {term}")))?;
                    if id == system::SPAM || id == system::TRASH {
                        anywhere = true;
                    }
                    tests.push(Box::new(move |m| m.has(&id)));
                }
                "is" => {
                    let (label, want) = match value.as_str() {
                        "unread" => (system::UNREAD, true),
                        "read" => (system::UNREAD, false),
                        "starred" => (system::STARRED, true),
                        "important" => (system::IMPORTANT, true),
                        _ => return Err(MockError::bad_request(&format!("Invalid query: {term}"))),
                    };
                    tests.push(Box::new(move |m| m.has(label) == want));
                }
                "category" => {
                    let category = GmailCategory::parse(&value)
                        .ok_or_else(|| MockError::bad_request(&format!("Invalid query: {term}")))?;
                    tests.push(Box::new(move |m| m.has(category.label())));
                }
                _ => return Err(MockError::bad_request(&format!("Invalid query: {term}"))),
            }
        }
        let mut out: Vec<&MockMessage> = self
            .messages
            .values()
            .filter(|m| label_ids.iter().all(|l| m.has(l)))
            .filter(|m| anywhere || !(m.has(system::SPAM) || m.has(system::TRASH)))
            .filter(|m| tests.iter().all(|t| t(m)))
            .collect();
        out.sort_by(|a, b| {
            b.internal_date_ms
                .cmp(&a.internal_date_ms)
                .then_with(|| b.id.cmp(&a.id))
        });
        Ok(out)
    }

    /// History records after `start` (`404` when expired), filtered.
    pub fn history_after(
        &self,
        start: u64,
        types: &[String],
        label_id: Option<&str>,
    ) -> MockResult<Vec<History>> {
        if start < self.oldest_history {
            return Err(MockError::not_found());
        }
        let wants = |t: &str| types.is_empty() || types.iter().any(|x| x == t);
        let has_label = |m: &Message| label_id.is_none_or(|l| m.label_ids.iter().any(|x| x == l));
        Ok(self
            .history
            .iter()
            .filter(|r| r.id.parse::<u64>().is_ok_and(|id| id > start))
            .filter_map(|r| {
                let mut r = r.clone();
                if !wants("messageAdded") {
                    r.messages_added.clear();
                }
                if !wants("messageDeleted") {
                    r.messages_deleted.clear();
                }
                if !wants("labelAdded") {
                    r.labels_added.clear();
                }
                if !wants("labelRemoved") {
                    r.labels_removed.clear();
                }
                if label_id.is_some() {
                    r.messages_added.retain(|e| has_label(&e.message));
                    r.messages_deleted.clear();
                    r.labels_added.retain(|e| {
                        has_label(&e.message)
                            || e.label_ids.iter().any(|l| Some(l.as_str()) == label_id)
                    });
                    r.labels_removed.retain(|e| {
                        e.label_ids.iter().any(|l| Some(l.as_str()) == label_id)
                            || has_label(&e.message)
                    });
                }
                let empty = r.messages_added.is_empty()
                    && r.messages_deleted.is_empty()
                    && r.labels_added.is_empty()
                    && r.labels_removed.is_empty();
                (!empty).then_some(r)
            })
            .collect())
    }

    // -- JSON views -----------------------------------------------------

    /// A message as `messages.get` answers in `format`.
    pub fn message_json(
        &self,
        m: &MockMessage,
        format: &str,
        metadata_headers: &[String],
    ) -> Value {
        let mut out = Message {
            id: m.id.clone(),
            thread_id: m.thread_id.clone(),
            label_ids: m.label_ids.clone(),
            snippet: Some(snippet(&m.raw)),
            history_id: Some(m.history_id.to_string()),
            internal_date: Some(m.internal_date_ms.to_string()),
            size_estimate: Some(m.raw.len() as u64),
            ..Message::default()
        };
        match format {
            "raw" => out.raw = Some(encode_base64url(&m.raw)),
            "metadata" | "full" => {
                let headers = raw_headers(&m.raw)
                    .into_iter()
                    .filter(|(n, _)| {
                        format == "full"
                            || metadata_headers.is_empty()
                            || metadata_headers.iter().any(|h| h.eq_ignore_ascii_case(n))
                    })
                    .map(|(name, value)| Header { name, value })
                    .collect();
                let mut payload = MessagePart {
                    part_id: String::new(),
                    mime_type: content_type(&m.raw),
                    headers,
                    ..MessagePart::default()
                };
                if format == "full" {
                    payload.parts = attachments(&m.raw)
                        .into_iter()
                        .enumerate()
                        .map(|(i, (name, mime, size))| MessagePart {
                            part_id: (i + 1).to_string(),
                            mime_type: mime,
                            filename: name,
                            body: Some(MessagePartBody {
                                attachment_id: Some(format!("att-{i}")),
                                size,
                                data: None,
                            }),
                            ..MessagePart::default()
                        })
                        .collect();
                }
                out.payload = Some(payload);
            }
            _ => {}
        }
        serde_json::to_value(out).unwrap_or(Value::Null)
    }

    /// An attachment's data (`att-<n>` = the n-th attachment).
    pub fn attachment_json(&self, message_id: &str, attachment_id: &str) -> MockResult<Value> {
        let m = self
            .messages
            .get(message_id)
            .ok_or_else(MockError::not_found)?;
        let n: u32 = attachment_id
            .strip_prefix("att-")
            .and_then(|n| n.parse().ok())
            .ok_or_else(MockError::not_found)?;
        let parsed = mail_parser::MessageParser::default()
            .parse(&m.raw)
            .ok_or_else(MockError::not_found)?;
        let part = parsed.attachment(n).ok_or_else(MockError::not_found)?;
        let data = part.contents();
        Ok(json!({ "size": data.len(), "data": encode_base64url(data) }))
    }

    /// Thread ids matching a list request, newest first.
    pub fn thread_ids(&self, messages: &[&MockMessage]) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for m in messages {
            if !out.contains(&m.thread_id) {
                out.push(m.thread_id.clone());
            }
        }
        out
    }

    /// The messages of a thread, oldest first.
    pub fn thread_messages(&self, thread_id: &str) -> Vec<&MockMessage> {
        let mut out: Vec<&MockMessage> = self
            .messages
            .values()
            .filter(|m| m.thread_id == thread_id)
            .collect();
        out.sort_by(|a, b| {
            a.internal_date_ms
                .cmp(&b.internal_date_ms)
                .then_with(|| a.id.cmp(&b.id))
        });
        out
    }
}

fn strip_header(raw: &[u8], name: &str) -> Vec<u8> {
    let text = String::from_utf8_lossy(raw).into_owned();
    let split = text.find("\r\n\r\n").map_or(text.len(), |i| i + 2);
    let (head, rest) = text.split_at(split);
    let mut out = String::new();
    let mut skipping = false;
    for line in head.split_inclusive("\r\n") {
        if line.starts_with([' ', '\t']) {
            if !skipping {
                out.push_str(line);
            }
            continue;
        }
        skipping = line
            .split_once(':')
            .is_some_and(|(n, _)| n.trim().eq_ignore_ascii_case(name));
        if !skipping {
            out.push_str(line);
        }
    }
    out.push_str(rest);
    out.into_bytes()
}

/// The `Message-ID` of a raw message (without angle brackets).
pub(crate) fn message_id_of(raw: &[u8]) -> Option<String> {
    header_value(&raw_headers(raw), "Message-ID").map(strip_id)
}

fn date_of(raw: &[u8]) -> i64 {
    mail_parser::MessageParser::default()
        .parse_headers(raw)
        .and_then(|m| m.date().map(|d| d.to_timestamp() * 1000))
        .unwrap_or_else(|| chrono::Utc::now().timestamp_millis())
}

fn snippet(raw: &[u8]) -> String {
    mail_parser::MessageParser::default()
        .parse(raw)
        .and_then(|m| {
            m.body_text(0)
                .map(|t| t.split_whitespace().collect::<Vec<_>>().join(" "))
        })
        .map(|t| t.chars().take(100).collect())
        .unwrap_or_default()
}

fn content_type(raw: &[u8]) -> String {
    header_value(&raw_headers(raw), "Content-Type").map_or_else(
        || "text/plain".to_owned(),
        |v| {
            v.split(';')
                .next()
                .unwrap_or_default()
                .trim()
                .to_ascii_lowercase()
        },
    )
}

fn attachments(raw: &[u8]) -> Vec<(String, String, u64)> {
    let Some(parsed) = mail_parser::MessageParser::default().parse(raw) else {
        return Vec::new();
    };
    parsed
        .attachments()
        .map(|part| {
            let mime = part.content_type().map_or_else(
                || "application/octet-stream".to_owned(),
                |c| match c.subtype() {
                    Some(sub) => format!("{}/{sub}", c.ctype()),
                    None => c.ctype().to_owned(),
                },
            );
            (
                part.attachment_name().unwrap_or_default().to_owned(),
                mime,
                part.contents().len() as u64,
            )
        })
        .collect()
}

/// `after:` / `before:` values: Unix seconds or `yyyy/mm/dd`.
fn parse_search_date(value: &str) -> Option<i64> {
    if let Ok(secs) = value.parse::<i64>() {
        return Some(secs);
    }
    let date = chrono::NaiveDate::parse_from_str(value, "%Y/%m/%d").ok()?;
    Some(date.and_hms_opt(0, 0, 0)?.and_utc().timestamp())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn raw(id: &str, extra: &str) -> Vec<u8> {
        format!("From: a@x\r\nTo: b@x\r\nMessage-ID: <{id}>\r\nDate: Thu, 01 Oct 2026 10:00:00 +0000\r\n{extra}Subject: s\r\n\r\nHello   world\r\n")
            .into_bytes()
    }

    fn s(list: &[&str]) -> Vec<String> {
        list.iter().map(|x| (*x).to_owned()).collect()
    }

    #[test]
    fn messages_threads_and_history() {
        let mut mb = Mailbox::new("me@x");
        let start = mb.history_id();
        let a = mb
            .add_message(raw("a@x", ""), &s(&["INBOX", "UNREAD"]), None)
            .unwrap();
        let b = mb
            .add_message(raw("b@x", "In-Reply-To: <a@x>\r\n"), &s(&["INBOX"]), None)
            .unwrap();
        assert_eq!(mb.message(&b).unwrap().thread_id, a);
        assert_eq!(mb.message(&a).unwrap().internal_date_ms, 1_790_848_800_000);
        mb.modify(&a, &s(&["STARRED"]), &s(&["UNREAD"])).unwrap();
        let before = mb.history_id();
        mb.modify(&a, &s(&["STARRED"]), &[]).unwrap();
        assert_eq!(mb.history_id(), before, "no change, no record");
        assert!(mb.modify(&a, &s(&["Label_404"]), &[]).is_err());
        mb.delete(&b).unwrap();
        let records = mb.history_after(start, &[], None).unwrap();
        assert_eq!(records.len(), 4);
        assert_eq!(records[2].labels_added[0].label_ids, ["STARRED"]);
        assert_eq!(records[2].labels_removed[0].label_ids, ["UNREAD"]);
        assert_eq!(records[3].messages_deleted[0].message.id, b);
        let only_added = mb
            .history_after(start, &s(&["messageAdded"]), None)
            .unwrap();
        assert_eq!(only_added.len(), 2);
        mb.expire_history();
        assert_eq!(mb.history_after(start, &[], None).unwrap_err().status, 404);
        assert!(
            mb.history_after(mb.history_id(), &[], None)
                .unwrap()
                .is_empty(),
            "{:?}",
            mb.history_after(mb.history_id(), &[], None).unwrap()
        );
    }

    #[test]
    fn search_terms() {
        let mut mb = Mailbox::new("me@x");
        let a = mb
            .add_message(
                raw("a@x", ""),
                &s(&["INBOX", "UNREAD", "CATEGORY_SOCIAL"]),
                None,
            )
            .unwrap();
        let t = mb
            .add_message(raw("t@x", ""), &s(&["TRASH"]), None)
            .unwrap();
        let ids = |v: Vec<&MockMessage>| v.into_iter().map(|m| m.id.clone()).collect::<Vec<_>>();
        assert_eq!(
            ids(mb.search(None, &[], false).unwrap()),
            std::slice::from_ref(&a)
        );
        assert_eq!(mb.search(None, &[], true).unwrap().len(), 2);
        assert_eq!(
            ids(mb.search(Some("rfc822msgid:<t@x>"), &[], false).unwrap()),
            [t]
        );
        assert_eq!(
            mb.search(Some("is:unread category:social"), &[], false)
                .unwrap()
                .len(),
            1
        );
        assert_eq!(
            mb.search(Some("after:1790848799"), &[], false)
                .unwrap()
                .len(),
            1
        );
        assert!(
            mb.search(Some("after:2026/10/02"), &[], false)
                .unwrap()
                .is_empty(),
            "{:?}",
            mb.search(Some("after:2026/10/02"), &[], false).unwrap()
        );
        assert_eq!(mb.search(Some("label:inbox"), &[], false).unwrap().len(), 1);
        assert_eq!(
            mb.search(Some("frobnicate:x"), &[], false)
                .unwrap_err()
                .status,
            400
        );
        assert_eq!(ids(mb.search(None, &s(&["UNREAD"]), false).unwrap()), [a]);
    }

    #[test]
    fn labels_validate_and_count() {
        let mut mb = Mailbox::new("me@x");
        let work = mb
            .create_label(Label {
                name: "Work".into(),
                ..Label::default()
            })
            .unwrap();
        assert_eq!(work.id, "Label_1");
        let dup = mb.create_label(Label {
            name: "work".into(),
            ..Label::default()
        });
        assert_eq!(dup.unwrap_err().status, 409);
        let bad_color = mb.create_label(Label {
            name: "Red".into(),
            color: Some(LabelColor {
                text_color: "#ffffff".into(),
                background_color: "#123456".into(),
            }),
            ..Label::default()
        });
        assert_eq!(bad_color.unwrap_err().status, 400);
        let m = mb
            .add_message(raw("a@x", ""), &s(&["INBOX", "UNREAD", "Label_1"]), None)
            .unwrap();
        let counted = mb.label_with_counts("Label_1").unwrap();
        assert_eq!(
            (counted.messages_total, counted.messages_unread),
            (Some(1), Some(1))
        );
        let renamed = mb
            .update_label("Label_1", &json!({"name": "Job"}), false)
            .unwrap();
        assert_eq!(renamed.name, "Job");
        assert_eq!(renamed.label_list_visibility.as_deref(), Some("labelShow"));
        assert_eq!(
            mb.update_label("INBOX", &json!({"name": "x"}), false)
                .unwrap_err()
                .status,
            400
        );
        mb.delete_label("Label_1").unwrap();
        assert!(
            !mb.message(&m)
                .unwrap()
                .label_ids
                .contains(&"Label_1".to_owned())
        );
        assert_eq!(mb.delete_label("Label_1").unwrap_err().status, 404);
    }

    #[test]
    fn send_needs_recipients_and_drops_bcc() {
        let mut mb = Mailbox::new("me@x");
        assert_eq!(
            mb.send(b"Subject: x\r\n\r\nx".to_vec(), None)
                .unwrap_err()
                .status,
            400
        );
        let id = mb
            .send(
                b"Bcc: hidden@x,\r\n other@x\r\nTo: b@x\r\nSubject: x\r\n\r\nx".to_vec(),
                None,
            )
            .unwrap();
        let stored = String::from_utf8(mb.message(&id).unwrap().raw.clone()).unwrap();
        assert!(!stored.contains("hidden") && !stored.contains("other@x"));
        assert!(stored.starts_with("To: b@x\r\n"));
        assert!(String::from_utf8_lossy(&mb.sent()[0]).contains("hidden@x"));
        assert_eq!(mb.message(&id).unwrap().label_ids, ["SENT"]);
    }

    #[test]
    fn json_views() {
        let mut mb = Mailbox::new("me@x");
        let raw = b"From: a@x\r\nTo: b@x\r\nSubject: files\r\nMIME-Version: 1.0\r\nContent-Type: multipart/mixed; boundary=XX\r\n\r\n--XX\r\nContent-Type: text/plain\r\n\r\nSee attached.\r\n--XX\r\nContent-Type: application/pdf; name=a.pdf\r\nContent-Disposition: attachment; filename=a.pdf\r\nContent-Transfer-Encoding: base64\r\n\r\nJVBERi0xLjQ=\r\n--XX--\r\n";
        let id = mb.add_message(raw.to_vec(), &s(&["INBOX"]), None).unwrap();
        let m = mb.message(&id).unwrap().clone();
        let minimal = mb.message_json(&m, "minimal", &[]);
        assert!(minimal.get("raw").is_none() && minimal.get("payload").is_none());
        assert_eq!(minimal["snippet"], "See attached.");
        let raw_view = mb.message_json(&m, "raw", &[]);
        assert_eq!(
            decode_base64url(raw_view["raw"].as_str().unwrap()).unwrap(),
            raw
        );
        let meta = mb.message_json(&m, "metadata", &s(&["Subject"]));
        assert_eq!(meta["payload"]["headers"].as_array().unwrap().len(), 1);
        let full = mb.message_json(&m, "full", &[]);
        assert_eq!(full["payload"]["mimeType"], "multipart/mixed");
        assert_eq!(full["payload"]["parts"][0]["filename"], "a.pdf");
        let att = mb.attachment_json(&id, "att-0").unwrap();
        assert_eq!(
            decode_base64url(att["data"].as_str().unwrap()).unwrap(),
            b"%PDF-1.4"
        );
        assert_eq!(mb.attachment_json(&id, "att-9").unwrap_err().status, 404);
    }

    #[test]
    fn fixtures_load() {
        let mb = Mailbox::from_fixture(super::super::fixtures::DEMO_MAILBOX).unwrap();
        assert_eq!(mb.email, "ada@gmail.example");
        assert!(mb.message_ids().len() >= 8);
        assert!(Mailbox::from_fixture(r#"{"emailAddress": "x", "messages": [{"id": "a", "rawText": "x", "labelIds": ["Nope"]}]}"#).is_err());
        assert!(Mailbox::from_fixture("{").is_err());
    }
}
