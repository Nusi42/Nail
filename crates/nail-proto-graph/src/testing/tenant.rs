//! The mock's in-memory tenant: mailboxes (the user's and shared ones)
//! with folders, messages, categories, settings, rules, overrides,
//! calendars and events, contacts, To Do; rooms, room lists, schedules;
//! the change log behind delta queries; fixture loading.

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;

use base64::Engine as _;
use serde_json::{Value, json};

use crate::mime;

/// An error the mock answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockError {
    /// HTTP status.
    pub status: u16,
    /// Graph's JSON error body.
    pub body: Value,
}

impl MockError {
    /// An error with Graph's body shape.
    pub fn new(status: u16, code: &str, message: &str) -> Self {
        Self {
            status,
            body: json!({"error": {"code": code, "message": message,
                "innerError": {"date": "2026-10-01T10:00:00", "request-id": "00000000-0000-0000-0000-00000000mock"}}}),
        }
    }

    /// `404 ErrorItemNotFound`.
    pub fn not_found(what: &str) -> Self {
        Self::new(
            404,
            "ErrorItemNotFound",
            &format!("The specified object was not found in the store. ({what})"),
        )
    }

    /// `400 ErrorInvalidRequest` (or another code).
    pub fn bad(code: &str, message: &str) -> Self {
        Self::new(400, code, message)
    }
}

/// A mock result.
pub type MockResult<T> = Result<T, MockError>;

/// Makes ids that look like Graph's (base64-ish with `=`, `-` and `_`,
/// so path encoding is exercised).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct IdGen {
    next: u64,
    /// The client asks for immutable ids (`Prefer: IdType="ImmutableId"`):
    /// a moved message keeps its id.
    pub immutable: bool,
}

impl IdGen {
    /// A new id of `kind` (`msg`, `fld`, `evt`, …).
    pub fn make(&mut self, kind: &str) -> String {
        self.next += 1;
        format!("AAMk-{kind}_{:05}=", self.next)
    }
}

/// A mail folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockFolder {
    /// Id.
    pub id: String,
    /// Display name.
    pub name: String,
    /// Parent (`None` for the root).
    pub parent: Option<String>,
    /// Well-known name (`inbox`, `msgfolderroot`, …).
    pub well_known: Option<String>,
    /// Hidden.
    pub hidden: bool,
}

/// A message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockMessage {
    /// Id (changes on moves).
    pub id: String,
    /// Folder id.
    pub folder: String,
    /// The MIME source.
    pub raw: Vec<u8>,
    /// Read.
    pub is_read: bool,
    /// Draft (MIME creates are drafts).
    pub is_draft: bool,
    /// `notFlagged`, `flagged`, `complete`.
    pub flag: String,
    /// Categories.
    pub categories: Vec<String>,
    /// `focused` / `other`.
    pub inference: String,
    /// Received (RFC 3339 UTC).
    pub received: String,
    /// Version (bumped on every change).
    pub change_key: u64,
    /// Conversation.
    pub conversation_id: String,
    /// `PidTagLastVerbExecuted`, when set.
    pub last_verb: Option<String>,
}

impl MockMessage {
    /// The message as Graph lists it.
    pub fn to_json(&self) -> Value {
        let header = |name: &str| mime::header(&self.raw, name).unwrap_or_default();
        let from = mime::addresses(&header("from"));
        let mut value = json!({
            "@odata.etag": format!("W/\"CQAAABYAAAA{:06}\"", self.change_key),
            "id": self.id,
            "changeKey": format!("CQAAABYAAAA{:06}", self.change_key),
            "conversationId": self.conversation_id,
            "parentFolderId": self.folder,
            "isRead": self.is_read,
            "isDraft": self.is_draft,
            "flag": {"flagStatus": self.flag},
            "categories": self.categories,
            "inferenceClassification": self.inference,
            "receivedDateTime": self.received,
            "internetMessageId": mime::message_id(&self.raw),
            "subject": header("subject"),
            "hasAttachments": String::from_utf8_lossy(&self.raw).contains("Content-Disposition: attachment"),
        });
        if let Some(address) = from.first() {
            value["from"] = json!({"emailAddress": {"address": address}});
        }
        value
    }
}

/// A To Do task with its parts.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MockTask {
    /// Its list.
    pub list: String,
    /// The task JSON.
    pub value: Value,
    /// Checklist items.
    pub checklist: Vec<Value>,
    /// Linked resources.
    pub links: Vec<Value>,
}

/// A message received through `sendMail`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SentMail {
    /// The mailbox it was sent from.
    pub mailbox: String,
    /// The MIME as uploaded (after base64 decoding), or as the mock
    /// rendered a draft built from parts.
    pub raw: Vec<u8>,
    /// How it was sent: `sendMail` (MIME) or `send` (a draft, M4c upload
    /// sessions).
    pub via: String,
}

/// A draft created from JSON (M4c: messages over 4 MB are built from
/// parts), with the attachments added so far.
#[derive(Debug, Clone, PartialEq)]
pub struct JsonDraft {
    /// The mailbox it is in.
    pub mailbox: String,
    /// The `message` resource as created.
    pub message: Value,
    /// Attachments: metadata and content.
    pub attachments: Vec<(Value, Vec<u8>)>,
}

/// An open attachment upload session.
#[derive(Debug, Clone, PartialEq)]
pub struct MockUpload {
    /// The mailbox of the draft.
    pub mailbox: String,
    /// The draft.
    pub message_id: String,
    /// The `AttachmentItem` of `createUploadSession`.
    pub item: Value,
    /// The announced size.
    pub size: usize,
    /// The bytes received so far.
    pub received: Vec<u8>,
    /// The ranges received (`start`, `end` inclusive).
    pub ranges: Vec<(u64, u64)>,
}

/// A finished upload (for assertions).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CompletedUpload {
    /// The draft.
    pub message_id: String,
    /// The file name.
    pub name: String,
    /// Its size.
    pub size: usize,
    /// How many range requests it took.
    pub ranges: usize,
}

/// A meeting answer or cancellation the server received.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MeetingAction {
    /// The event.
    pub event_id: String,
    /// `accept`, `tentativelyAccept`, `decline`, `cancel`.
    pub action: String,
    /// The request body.
    pub body: Value,
}

/// A change of a collection (delta queries).
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Change {
    pub seq: u64,
    pub collection: String,
    pub id: String,
}

/// One mailbox.
#[derive(Debug, Clone, PartialEq)]
pub struct Mailbox {
    /// Address (lowercase).
    pub address: String,
    /// Display name.
    pub display_name: String,
    /// Directory id.
    pub user_id: String,
    /// Zone.
    pub time_zone: String,
    /// Mail folders by id.
    pub folders: BTreeMap<String, MockFolder>,
    /// Messages by id.
    pub messages: BTreeMap<String, MockMessage>,
    /// Master categories.
    pub categories: Vec<Value>,
    /// `automaticRepliesSetting`.
    pub automatic_replies: Value,
    /// Inbox rules.
    pub rules: Vec<Value>,
    /// Focused Inbox overrides.
    pub overrides: Vec<Value>,
    /// Calendars.
    pub calendars: Vec<Value>,
    /// Events by id (series masters, singles, exceptions).
    pub events: BTreeMap<String, Value>,
    /// The calendar of each event.
    pub event_calendar: BTreeMap<String, String>,
    /// The default contacts folder's id.
    pub default_contacts: String,
    /// Contact folders below the default one.
    pub contact_folders: Vec<Value>,
    /// Contacts by id.
    pub contacts: BTreeMap<String, Value>,
    /// The folder of each contact.
    pub contact_folder: BTreeMap<String, String>,
    /// To Do lists.
    pub todo_lists: Vec<Value>,
    /// To Do tasks by id.
    pub tasks: BTreeMap<String, MockTask>,
    pub(crate) seq: u64,
    pub(crate) changes: Vec<Change>,
    pub(crate) expired_below: u64,
}

/// The tenant.
#[derive(Debug, Clone, PartialEq)]
pub struct Tenant {
    /// The signed-in user's address (lowercase).
    pub me: String,
    /// Mailboxes by lowercase address.
    pub mailboxes: BTreeMap<String, Mailbox>,
    /// Mailboxes the user may not open (`403`).
    pub denied: BTreeSet<String>,
    /// Mailboxes on-premises (`MailboxNotEnabledForRESTAPI`).
    pub on_premises: BTreeSet<String>,
    /// Rooms (place JSON).
    pub rooms: Vec<Value>,
    /// Room lists (place JSON) with their rooms' addresses.
    pub room_lists: Vec<(Value, Vec<String>)>,
    /// `Place.Read.All` missing: places answer `403`.
    pub places_forbidden: bool,
    /// Busy items per address (`getSchedule`).
    pub schedules: BTreeMap<String, Vec<Value>>,
    /// Messages sent.
    pub sent: Vec<SentMail>,
    /// Meeting answers and cancellations.
    pub meeting_actions: Vec<MeetingAction>,
    /// Ids.
    pub ids: IdGen,
    /// Drafts created from JSON, by message id (M4c).
    pub json_drafts: BTreeMap<String, JsonDraft>,
    /// Open upload sessions, by session id (M4c).
    pub uploads: BTreeMap<String, MockUpload>,
    /// Finished uploads (M4c).
    pub completed_uploads: Vec<CompletedUpload>,
}

/// The well-known folders every mailbox has, with display names.
pub const WELL_KNOWN: [(&str, &str); 7] = [
    ("inbox", "Inbox"),
    ("sentitems", "Sent Items"),
    ("drafts", "Drafts"),
    ("deleteditems", "Deleted Items"),
    ("junkemail", "Junk Email"),
    ("archive", "Archive"),
    ("outbox", "Outbox"),
];

impl Mailbox {
    /// A mailbox with the well-known folders, a default calendar, the
    /// default contacts folder and a default To Do list.
    pub fn new(address: &str, display_name: &str, ids: &mut IdGen) -> Self {
        let address = address.to_ascii_lowercase();
        let mut folders = BTreeMap::new();
        let root = ids.make("root");
        folders.insert(
            root.clone(),
            MockFolder {
                id: root.clone(),
                name: "Top of Information Store".into(),
                parent: None,
                well_known: Some("msgfolderroot".into()),
                hidden: false,
            },
        );
        for (wk, name) in WELL_KNOWN {
            let id = ids.make("fld");
            folders.insert(
                id.clone(),
                MockFolder {
                    id,
                    name: name.into(),
                    parent: Some(root.clone()),
                    well_known: Some(wk.into()),
                    hidden: false,
                },
            );
        }
        let calendar = ids.make("cal");
        let tasks = ids.make("lst");
        let flagged = ids.make("lst");
        Self {
            user_id: format!("00000000-0000-0000-0000-{:012}", address.len()),
            address,
            display_name: display_name.to_owned(),
            time_zone: "W. Europe Standard Time".into(),
            folders,
            messages: BTreeMap::new(),
            categories: Vec::new(),
            automatic_replies: json!({"status": "disabled", "externalAudience": "all",
                "internalReplyMessage": "", "externalReplyMessage": "",
                "scheduledStartDateTime": {"dateTime": "2026-10-01T00:00:00.0000000", "timeZone": "UTC"},
                "scheduledEndDateTime": {"dateTime": "2026-10-02T00:00:00.0000000", "timeZone": "UTC"}}),
            rules: Vec::new(),
            overrides: Vec::new(),
            calendars: vec![json!({"id": calendar, "name": "Calendar", "color": "auto",
                "changeKey": "cal1", "canEdit": true, "isDefaultCalendar": true})],
            events: BTreeMap::new(),
            event_calendar: BTreeMap::new(),
            default_contacts: ids.make("cfd"),
            contact_folders: Vec::new(),
            contacts: BTreeMap::new(),
            contact_folder: BTreeMap::new(),
            todo_lists: vec![
                json!({"id": tasks, "displayName": "Tasks", "wellknownListName": "defaultList",
                    "isOwner": true, "isShared": false, "@odata.etag": "W/\"list1\""}),
                json!({"id": flagged, "displayName": "Flagged email", "wellknownListName": "flaggedEmails",
                    "isOwner": true, "isShared": false, "@odata.etag": "W/\"list2\""}),
            ],
            tasks: BTreeMap::new(),
            seq: 1,
            changes: Vec::new(),
            expired_below: 0,
        }
    }

    /// Records a change of `id` in `collection`.
    pub fn touch(&mut self, collection: &str, id: &str) {
        self.seq += 1;
        self.changes.push(Change {
            seq: self.seq,
            collection: collection.to_owned(),
            id: id.to_owned(),
        });
    }

    /// The current change sequence (a delta token).
    pub fn seq(&self) -> u64 {
        self.seq
    }

    /// Forgets the changes so far: older delta links answer `410`.
    pub fn expire_deltas(&mut self) {
        self.seq += 1;
        self.expired_below = self.seq;
        self.changes.clear();
    }

    /// Whether delta token `token` is still valid.
    pub(crate) fn token_valid(&self, token: u64) -> bool {
        token >= self.expired_below && token <= self.seq
    }

    /// The ids changed in `collection` after `token` (up to `upto`),
    /// oldest change first, each once.
    pub(crate) fn changed_since(&self, collection: &str, token: u64, upto: u64) -> Vec<String> {
        let mut out: Vec<String> = Vec::new();
        for change in self
            .changes
            .iter()
            .filter(|c| c.seq > token && c.seq <= upto && c.collection == collection)
        {
            out.retain(|id| *id != change.id);
            out.push(change.id.clone());
        }
        out
    }

    /// The id of a folder given by id or well-known name.
    pub fn folder_id(&self, id_or_name: &str) -> Option<String> {
        if self.folders.contains_key(id_or_name) {
            return Some(id_or_name.to_owned());
        }
        self.folders
            .values()
            .find(|f| {
                f.well_known
                    .as_deref()
                    .is_some_and(|w| w.eq_ignore_ascii_case(id_or_name))
            })
            .map(|f| f.id.clone())
    }

    /// The root folder's id.
    pub fn root(&self) -> String {
        self.folder_id("msgfolderroot").unwrap_or_default()
    }

    /// A folder as Graph lists it.
    pub fn folder_json(&self, folder: &MockFolder) -> Value {
        let children = self
            .folders
            .values()
            .filter(|f| f.parent.as_deref() == Some(&folder.id))
            .count();
        let in_folder = self.messages.values().filter(|m| m.folder == folder.id);
        let total = in_folder.clone().count();
        let unread = in_folder.filter(|m| !m.is_read).count();
        json!({
            "id": folder.id,
            "displayName": folder.name,
            "parentFolderId": folder.parent.clone().unwrap_or_default(),
            "childFolderCount": children,
            "totalItemCount": total,
            "unreadItemCount": unread,
            "isHidden": folder.hidden,
        })
    }

    /// Adds a folder below `parent` (id or well-known name; `None` = the
    /// root); answers its id.
    pub fn add_folder(
        &mut self,
        ids: &mut IdGen,
        parent: Option<&str>,
        name: &str,
    ) -> MockResult<String> {
        let parent = match parent {
            Some(p) => self.folder_id(p).ok_or_else(|| MockError::not_found(p))?,
            None => self.root(),
        };
        if self
            .folders
            .values()
            .any(|f| f.parent.as_deref() == Some(&parent) && f.name.eq_ignore_ascii_case(name))
        {
            return Err(MockError::new(
                409,
                "ErrorFolderExists",
                "A folder with the specified name already exists.",
            ));
        }
        let id = ids.make("fld");
        self.folders.insert(
            id.clone(),
            MockFolder {
                id: id.clone(),
                name: name.to_owned(),
                parent: Some(parent),
                well_known: None,
                hidden: false,
            },
        );
        self.touch("folders", &id);
        Ok(id)
    }

    /// Renames a folder.
    pub fn rename_folder(&mut self, id: &str, name: &str) -> MockResult<()> {
        let id = self.folder_id(id).ok_or_else(|| MockError::not_found(id))?;
        if let Some(folder) = self.folders.get_mut(&id) {
            name.clone_into(&mut folder.name);
        }
        self.touch("folders", &id);
        Ok(())
    }

    /// Moves a folder (with its subfolders) below `parent` (an id or a
    /// well-known name such as `msgfolderroot`).
    pub fn move_folder(&mut self, id: &str, parent: &str) -> MockResult<()> {
        let id = self.folder_id(id).ok_or_else(|| MockError::not_found(id))?;
        let parent = self
            .folder_id(parent)
            .ok_or_else(|| MockError::not_found(parent))?;
        let name = self
            .folders
            .get(&id)
            .map(|f| f.name.clone())
            .unwrap_or_default();
        if self.folders.values().any(|f| {
            f.id != id && f.parent.as_deref() == Some(&parent) && f.name.eq_ignore_ascii_case(&name)
        }) {
            return Err(MockError::new(
                409,
                "ErrorFolderExists",
                "A folder with the specified name already exists.",
            ));
        }
        if let Some(folder) = self.folders.get_mut(&id) {
            folder.parent = Some(parent);
        }
        self.touch("folders", &id);
        Ok(())
    }

    /// Deletes a folder with its subfolders and messages.
    pub fn remove_folder(&mut self, id: &str) -> MockResult<()> {
        let id = self.folder_id(id).ok_or_else(|| MockError::not_found(id))?;
        if self
            .folders
            .get(&id)
            .is_some_and(|f| f.well_known.is_some())
        {
            return Err(MockError::new(
                403,
                "ErrorDeleteDistinguishedFolder",
                "Distinguished folders cannot be deleted.",
            ));
        }
        let children: Vec<String> = self
            .folders
            .values()
            .filter(|f| f.parent.as_deref() == Some(&id))
            .map(|f| f.id.clone())
            .collect();
        for child in children {
            self.remove_folder(&child)?;
        }
        let messages: Vec<String> = self
            .messages
            .values()
            .filter(|m| m.folder == id)
            .map(|m| m.id.clone())
            .collect();
        for m in messages {
            self.messages.remove(&m);
            self.touch(&format!("msgs:{id}"), &m);
        }
        self.folders.remove(&id);
        self.touch("folders", &id);
        Ok(())
    }

    /// Delivers a raw message into a folder; answers its id.
    pub fn add_message(
        &mut self,
        ids: &mut IdGen,
        folder: &str,
        raw: Vec<u8>,
        is_read: bool,
        is_draft: bool,
    ) -> MockResult<String> {
        let folder = self
            .folder_id(folder)
            .ok_or_else(|| MockError::not_found(folder))?;
        let id = ids.make("msg");
        let received = mime::header(&raw, "date")
            .and_then(|d| chrono::DateTime::parse_from_rfc2822(&d).ok())
            .map_or_else(
                || "2026-10-01T08:00:00Z".to_owned(),
                |d| {
                    d.with_timezone(&chrono::Utc)
                        .format("%Y-%m-%dT%H:%M:%SZ")
                        .to_string()
                },
            );
        let conversation_id = mime::header(&raw, "subject").map_or_else(
            || id.clone(),
            |s| {
                let s = s
                    .trim_start_matches("Re: ")
                    .trim_start_matches("RE: ")
                    .to_owned();
                format!("AAQk-conv-{}", s.len())
            },
        );
        self.messages.insert(
            id.clone(),
            MockMessage {
                id: id.clone(),
                folder: folder.clone(),
                raw,
                is_read,
                is_draft,
                flag: "notFlagged".into(),
                categories: Vec::new(),
                inference: "focused".into(),
                received,
                change_key: 1,
                conversation_id,
                last_verb: None,
            },
        );
        self.touch(&format!("msgs:{folder}"), &id);
        Ok(id)
    }

    /// Changes a message with `f` (and records the change).
    pub fn change_message(&mut self, id: &str, f: impl FnOnce(&mut MockMessage)) -> MockResult<()> {
        let message = self
            .messages
            .get_mut(id)
            .ok_or_else(|| MockError::not_found(id))?;
        f(message);
        message.change_key += 1;
        let folder = message.folder.clone();
        self.touch(&format!("msgs:{folder}"), id);
        Ok(())
    }

    /// Moves (or copies) a message; answers the new id.
    pub fn move_message(
        &mut self,
        ids: &mut IdGen,
        id: &str,
        to: &str,
        copy: bool,
    ) -> MockResult<String> {
        let to = self
            .folder_id(to)
            .ok_or_else(|| MockError::new(400, "ErrorInvalidIdMalformed", "Id is malformed."))?;
        let mut message = self
            .messages
            .get(id)
            .cloned()
            .ok_or_else(|| MockError::not_found(id))?;
        let new_id = if !copy && ids.immutable {
            id.to_owned()
        } else {
            ids.make("msg")
        };
        if !copy {
            self.messages.remove(id);
            self.touch(&format!("msgs:{}", message.folder), id);
        }
        message.id.clone_from(&new_id);
        message.folder.clone_from(&to);
        message.change_key += 1;
        self.messages.insert(new_id.clone(), message);
        self.touch(&format!("msgs:{to}"), &new_id);
        Ok(new_id)
    }

    /// Deletes a message for good.
    pub fn remove_message(&mut self, id: &str) -> MockResult<()> {
        let message = self
            .messages
            .remove(id)
            .ok_or_else(|| MockError::not_found(id))?;
        self.touch(&format!("msgs:{}", message.folder), id);
        Ok(())
    }

    /// Adds an event to a calendar (`None` = the default); answers its id.
    pub fn add_event(
        &mut self,
        ids: &mut IdGen,
        calendar: Option<&str>,
        mut event: Value,
    ) -> MockResult<String> {
        let calendar = match calendar {
            Some(c) => self
                .calendars
                .iter()
                .find(|x| x["id"] == c)
                .map(|x| x["id"].as_str().unwrap_or_default().to_owned())
                .ok_or_else(|| MockError::not_found(c))?,
            None => self.calendars[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        };
        let id = ids.make("evt");
        let me = json!({"emailAddress": {"name": self.display_name, "address": self.address}});
        let object = event
            .as_object_mut()
            .ok_or_else(|| MockError::bad("ErrorInvalidRequest", "not an object"))?;
        object.insert("id".into(), json!(id));
        object.insert("changeKey".into(), json!("DwAAAB1"));
        object.entry("iCalUId").or_insert_with(|| {
            // Unique per event, shaped like Exchange's (a fixed prefix
            // and the item's bytes in hex).
            let hex: String = id.bytes().fold(String::new(), |mut out, b| {
                let _ = write!(out, "{b:02X}");
                out
            });
            json!(format!("040000008200E00074C5B7101A82E008{hex}"))
        });
        let kind = if object.get("recurrence").is_some_and(|r| !r.is_null()) {
            "seriesMaster"
        } else {
            "singleInstance"
        };
        object.entry("type").or_insert_with(|| json!(kind));
        object.entry("organizer").or_insert(me);
        object.entry("isOrganizer").or_insert(json!(true));
        object
            .entry("responseStatus")
            .or_insert(json!({"response": "organizer"}));
        object.entry("showAs").or_insert(json!("busy"));
        object.entry("isCancelled").or_insert(json!(false));
        object
            .entry("lastModifiedDateTime")
            .or_insert(json!("2026-10-01T08:00:00Z"));
        if let Some(start) = object.get("start").and_then(|s| s.get("timeZone")).cloned() {
            object.entry("originalStartTimeZone").or_insert(start);
        }
        self.events.insert(id.clone(), event);
        self.event_calendar.insert(id.clone(), calendar.clone());
        self.touch(&format!("cal:{calendar}"), &id);
        Ok(id)
    }

    /// Records a change of an event in its calendar (exceptions count for
    /// their series' calendar).
    pub fn touch_event(&mut self, id: &str) {
        let calendar = self.event_calendar.get(id).cloned().or_else(|| {
            id.split(".occ.")
                .next()
                .and_then(|m| self.event_calendar.get(m).cloned())
        });
        if let Some(calendar) = calendar {
            self.touch(&format!("cal:{calendar}"), id);
        }
    }

    /// Bumps an event's change key.
    pub fn bump_event(&mut self, id: &str) {
        if let Some(event) = self.events.get_mut(id) {
            let next = event["changeKey"]
                .as_str()
                .unwrap_or("DwAAAB0")
                .trim_start_matches("DwAAAB")
                .parse::<u64>()
                .unwrap_or(0)
                + 1;
            event["changeKey"] = json!(format!("DwAAAB{next}"));
        }
    }

    /// Adds a contact (`None` = the default folder); answers its id.
    pub fn add_contact(
        &mut self,
        ids: &mut IdGen,
        folder: Option<&str>,
        mut contact: Value,
    ) -> MockResult<String> {
        let folder = folder.map_or_else(|| self.default_contacts.clone(), str::to_owned);
        if folder != self.default_contacts
            && !self.contact_folders.iter().any(|f| f["id"] == folder)
        {
            return Err(MockError::not_found(&folder));
        }
        let id = ids.make("con");
        contact["id"] = json!(id);
        contact["changeKey"] = json!("EQAAAB1");
        contact["parentFolderId"] = json!(folder);
        self.contacts.insert(id.clone(), contact);
        self.contact_folder.insert(id.clone(), folder.clone());
        self.touch(&format!("contacts:{folder}"), &id);
        Ok(id)
    }

    /// Adds a contact folder below the default one; answers its id.
    pub fn add_contact_folder(&mut self, ids: &mut IdGen, name: &str) -> String {
        let id = ids.make("cfd");
        self.contact_folders
            .push(json!({"id": id, "displayName": name, "parentFolderId": self.default_contacts}));
        id
    }

    /// Adds a To Do task to a list (`None` = the default list); answers its
    /// id.
    pub fn add_task(
        &mut self,
        ids: &mut IdGen,
        list: Option<&str>,
        mut task: Value,
        checklist: Vec<Value>,
    ) -> MockResult<String> {
        let list = match list {
            Some(l) if self.todo_lists.iter().any(|x| x["id"] == l) => l.to_owned(),
            Some(l) => return Err(MockError::not_found(l)),
            None => self.todo_lists[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned(),
        };
        let id = ids.make("tsk");
        task["id"] = json!(id);
        task["@odata.etag"] = json!("W/\"etag1\"");
        if let Some(o) = task.as_object_mut() {
            o.entry("status").or_insert(json!("notStarted"));
        }
        let checklist = checklist
            .into_iter()
            .map(|mut c| {
                c["id"] = json!(ids.make("chk"));
                c
            })
            .collect();
        self.tasks.insert(
            id.clone(),
            MockTask {
                list: list.clone(),
                value: task,
                checklist,
                links: Vec::new(),
            },
        );
        self.touch(&format!("todo:{list}"), &id);
        Ok(id)
    }

    /// Bumps a task's etag and records the change.
    pub fn touch_task(&mut self, id: &str) {
        if let Some(task) = self.tasks.get_mut(id) {
            let next = task.value["@odata.etag"]
                .as_str()
                .and_then(|e| {
                    e.trim_start_matches("W/\"etag")
                        .trim_end_matches('"')
                        .parse::<u64>()
                        .ok()
                })
                .unwrap_or(0)
                + 1;
            task.value["@odata.etag"] = json!(format!("W/\"etag{next}\""));
            let list = task.list.clone();
            self.touch(&format!("todo:{list}"), id);
        }
    }
}

impl Tenant {
    /// A tenant with one mailbox for `email`.
    pub fn new(email: &str) -> Self {
        let mut ids = IdGen::default();
        let me = email.to_ascii_lowercase();
        let mailbox = Mailbox::new(&me, "Ada Lovelace", &mut ids);
        Self {
            me: me.clone(),
            mailboxes: BTreeMap::from([(me, mailbox)]),
            denied: BTreeSet::new(),
            on_premises: BTreeSet::new(),
            rooms: Vec::new(),
            room_lists: Vec::new(),
            places_forbidden: false,
            schedules: BTreeMap::new(),
            sent: Vec::new(),
            meeting_actions: Vec::new(),
            ids,
            json_drafts: BTreeMap::new(),
            uploads: BTreeMap::new(),
            completed_uploads: Vec::new(),
        }
    }

    /// Adds a mailbox (a shared one, say) the user can open.
    pub fn add_mailbox(&mut self, address: &str, display_name: &str) {
        let mailbox = Mailbox::new(address, display_name, &mut self.ids);
        self.mailboxes.insert(address.to_ascii_lowercase(), mailbox);
    }

    /// The user's mailbox.
    pub fn own(&mut self) -> &mut Mailbox {
        let me = self.me.clone();
        self.mailbox(&me)
            .unwrap_or_else(|_| unreachable!("the own mailbox exists"))
    }

    /// A mailbox by address, as the user may open it.
    pub fn mailbox(&mut self, address: &str) -> MockResult<&mut Mailbox> {
        let address = address.to_ascii_lowercase();
        if self.denied.contains(&address) {
            return Err(MockError::new(
                403,
                "ErrorAccessDenied",
                "Access is denied. Check credentials and try again.",
            ));
        }
        if self.on_premises.contains(&address) {
            return Err(MockError::new(
                404,
                "MailboxNotEnabledForRESTAPI",
                "The mailbox is either inactive, soft-deleted, or is hosted on-premise.",
            ));
        }
        self.mailboxes.get_mut(&address).ok_or_else(|| {
            MockError::new(
                404,
                "ErrorInvalidUser",
                &format!("The requested user '{address}' is invalid."),
            )
        })
    }

    /// A tenant from fixture JSON (see `fixtures/tenant-demo.json`).
    pub fn from_fixture(text: &str) -> Result<Self, String> {
        let value: Value = serde_json::from_str(text).map_err(|e| e.to_string())?;
        let me = value["me"].as_str().ok_or("fixture without `me`")?;
        let mut tenant = Self::new(me);
        tenant.mailboxes.clear();
        for mb in value["mailboxes"]
            .as_array()
            .ok_or("fixture without `mailboxes`")?
        {
            load_mailbox(&mut tenant, mb)?;
        }
        if !tenant.mailboxes.contains_key(&tenant.me) {
            return Err("the fixture lacks the user's own mailbox".into());
        }
        for d in value["denied"].as_array().into_iter().flatten() {
            tenant
                .denied
                .insert(d.as_str().unwrap_or_default().to_ascii_lowercase());
        }
        for d in value["onPremises"].as_array().into_iter().flatten() {
            tenant
                .on_premises
                .insert(d.as_str().unwrap_or_default().to_ascii_lowercase());
        }
        tenant.rooms = value["rooms"].as_array().cloned().unwrap_or_default();
        for list in value["roomLists"].as_array().into_iter().flatten() {
            let rooms = list["rooms"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(Value::as_str)
                .map(str::to_ascii_lowercase)
                .collect();
            tenant.room_lists.push((list["place"].clone(), rooms));
        }
        if let Some(schedules) = value["schedules"].as_object() {
            for (address, items) in schedules {
                tenant.schedules.insert(
                    address.to_ascii_lowercase(),
                    items.as_array().cloned().unwrap_or_default(),
                );
            }
        }
        Ok(tenant)
    }
}

fn load_mailbox(tenant: &mut Tenant, mb: &Value) -> Result<(), String> {
    let address = mb["address"].as_str().ok_or("mailbox without address")?;
    let mut mailbox = Mailbox::new(
        address,
        mb["displayName"].as_str().unwrap_or(address),
        &mut tenant.ids,
    );
    let ids = &mut tenant.ids;
    if let Some(zone) = mb["timeZone"].as_str() {
        zone.clone_into(&mut mailbox.time_zone);
    }
    if mb["noArchive"].as_bool() == Some(true)
        && let Some(archive) = mailbox.folder_id("archive")
    {
        mailbox.folders.remove(&archive);
    }
    // Folders by name so messages can name them ("Inbox/Receipts").
    let mut named: BTreeMap<String, String> = BTreeMap::new();
    for f in mb["folders"].as_array().into_iter().flatten() {
        let name = f["name"].as_str().ok_or("folder without name")?;
        let parent = f["parent"]
            .as_str()
            .map(|p| named.get(p).cloned().unwrap_or_else(|| p.to_owned()));
        let id = mailbox
            .add_folder(ids, parent.as_deref(), name)
            .map_err(|e| format!("{e:?}"))?;
        let path = match f["parent"].as_str() {
            Some(p) => format!("{p}/{name}"),
            None => name.to_owned(),
        };
        named.insert(path, id);
    }
    for m in mb["messages"].as_array().into_iter().flatten() {
        let folder = m["folder"].as_str().unwrap_or("inbox");
        let folder = named
            .get(folder)
            .cloned()
            .unwrap_or_else(|| folder.to_owned());
        let raw = match (m["raw"].as_str(), m["rawBase64"].as_str()) {
            (Some(text), _) => text.replace('\n', "\r\n").into_bytes(),
            (None, Some(b64)) => base64::engine::general_purpose::STANDARD
                .decode(b64)
                .map_err(|e| e.to_string())?,
            _ => return Err("message without raw".into()),
        };
        let id = mailbox
            .add_message(
                ids,
                &folder,
                raw,
                m["isRead"].as_bool().unwrap_or(false),
                false,
            )
            .map_err(|e| format!("{e:?}"))?;
        let message = mailbox.messages.get_mut(&id).ok_or("message vanished")?;
        if let Some(flag) = m["flag"].as_str() {
            flag.clone_into(&mut message.flag);
        }
        if let Some(inference) = m["inference"].as_str() {
            inference.clone_into(&mut message.inference);
        }
        message.categories = m["categories"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(Value::as_str)
            .map(str::to_owned)
            .collect();
    }
    mailbox.categories = mb["categories"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|c| {
            let mut c = c.clone();
            c["id"] = json!(ids.make("cat"));
            c
        })
        .collect();
    if let Some(oof) = mb.get("automaticReplies").filter(|v| v.is_object()) {
        mailbox.automatic_replies = oof.clone();
    }
    mailbox.rules = mb["rules"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| {
            let mut r = r.clone();
            r["id"] = json!(ids.make("rul"));
            r
        })
        .collect();
    for (i, cal) in mb["calendars"].as_array().into_iter().flatten().enumerate() {
        let calendar_id = if i == 0 {
            mailbox.calendars[0]["name"] = cal["name"].clone();
            mailbox.calendars[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        } else {
            let id = ids.make("cal");
            mailbox.calendars.push(json!({"id": id, "name": cal["name"], "color": cal["color"].as_str().unwrap_or("auto"),
                "changeKey": "cal1", "canEdit": cal["canEdit"].as_bool().unwrap_or(true), "isDefaultCalendar": false}));
            id
        };
        for e in cal["events"].as_array().into_iter().flatten() {
            mailbox
                .add_event(ids, Some(&calendar_id), e.clone())
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    for c in mb["contacts"].as_array().into_iter().flatten() {
        mailbox
            .add_contact(ids, None, c.clone())
            .map_err(|e| format!("{e:?}"))?;
    }
    for f in mb["contactFolders"].as_array().into_iter().flatten() {
        let id = mailbox.add_contact_folder(ids, f["displayName"].as_str().unwrap_or_default());
        for c in f["contacts"].as_array().into_iter().flatten() {
            mailbox
                .add_contact(ids, Some(&id), c.clone())
                .map_err(|e| format!("{e:?}"))?;
        }
    }
    for (i, list) in mb["todoLists"].as_array().into_iter().flatten().enumerate() {
        let list_id = if list["wellknownListName"] == "defaultList" {
            mailbox.todo_lists[0]["id"]
                .as_str()
                .unwrap_or_default()
                .to_owned()
        } else {
            let id = ids.make("lst");
            mailbox
                .todo_lists
                .push(json!({"id": id, "displayName": list["displayName"],
                "wellknownListName": "none", "isOwner": true, "isShared": false,
                "@odata.etag": format!("W/\"list{}\"", i + 3)}));
            id
        };
        for t in list["tasks"].as_array().into_iter().flatten() {
            let mut task = t.clone();
            let checklist = task
                .as_object_mut()
                .and_then(|o| o.remove("checklistItems"))
                .and_then(|c| c.as_array().cloned())
                .unwrap_or_default();
            let links = task
                .as_object_mut()
                .and_then(|o| o.remove("linkedResources"))
                .and_then(|c| c.as_array().cloned())
                .unwrap_or_default();
            let id = mailbox
                .add_task(ids, Some(&list_id), task, checklist)
                .map_err(|e| format!("{e:?}"))?;
            if let Some(t) = mailbox.tasks.get_mut(&id) {
                t.links = links;
            }
        }
    }
    // Fixture content is the starting point: deltas begin after it.
    mailbox.changes.clear();
    let key = mailbox.address.clone();
    tenant.mailboxes.insert(key, mailbox);
    Ok(())
}

/// Merges `patch` into `target` (top-level keys replaced, `null` kept as
/// `null`, like Graph's `PATCH`).
pub(super) fn merge(target: &mut Value, patch: &Value) {
    if let (Some(t), Some(p)) = (target.as_object_mut(), patch.as_object()) {
        for (k, v) in p {
            t.insert(k.clone(), v.clone());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mailboxes_have_well_known_folders_and_changes() {
        let mut tenant = Tenant::new("Ada@Contoso.example");
        assert_eq!(tenant.me, "ada@contoso.example");
        let ids = &mut tenant.ids;
        let mb = tenant.mailboxes.get_mut("ada@contoso.example").unwrap();
        let inbox = mb.folder_id("INBOX").unwrap();
        assert_eq!(
            mb.folders[&inbox].parent.as_deref(),
            Some(mb.root().as_str())
        );
        let start = mb.seq();
        let id = mb
            .add_message(
                ids,
                "inbox",
                b"Subject: a\r\nDate: Thu, 01 Oct 2026 10:00:00 +0200\r\n\r\nx".to_vec(),
                false,
                false,
            )
            .unwrap();
        assert_eq!(mb.messages[&id].received, "2026-10-01T08:00:00Z");
        ids.immutable = false;
        let moved = mb.move_message(ids, &id, "archive", false).unwrap();
        assert_ne!(moved, id);
        let now = mb.seq();
        assert_eq!(
            mb.changed_since(&format!("msgs:{inbox}"), start, now),
            [id.as_str()]
        );
        let archive = mb.folder_id("archive").unwrap();
        assert_eq!(
            mb.changed_since(&format!("msgs:{archive}"), start, now),
            [moved.as_str()]
        );
        assert_eq!(
            mb.changed_since(&format!("msgs:{archive}"), now, now).len(),
            0
        );
        assert!(mb.token_valid(start));
        mb.expire_deltas();
        assert!(!mb.token_valid(start));
        assert!(mb.token_valid(mb.seq()), "a fresh token is valid");
        assert!(
            mb.remove_folder("inbox").is_err(),
            "well-known folders stay"
        );
        let f = mb.add_folder(ids, Some("inbox"), "Receipts").unwrap();
        assert_eq!(
            mb.add_folder(ids, Some("inbox"), "receipts")
                .unwrap_err()
                .status,
            409
        );
        mb.remove_folder(&f).unwrap();
        assert!(!mb.folders.contains_key(&f));
        tenant.denied.insert("ceo@contoso.example".into());
        assert_eq!(
            tenant.mailbox("ceo@contoso.example").unwrap_err().status,
            403
        );
        assert_eq!(tenant.mailbox("x@contoso.example").unwrap_err().status, 404);
    }

    #[test]
    fn demo_fixture_loads() {
        let tenant = Tenant::from_fixture(super::super::fixtures::DEMO_TENANT).unwrap();
        assert!(tenant.mailboxes.len() >= 2);
        let own = &tenant.mailboxes[&tenant.me];
        assert!(own.messages.len() >= 3);
        assert!(!own.events.is_empty() && !own.contacts.is_empty() && !own.tasks.is_empty());
        assert!(own.changes.is_empty(), "{:?}", own.changes);
        assert!(Tenant::from_fixture("{}").is_err());
        assert!(Tenant::from_fixture(r#"{"me": "a@b.example", "mailboxes": []}"#).is_err());
    }
}
