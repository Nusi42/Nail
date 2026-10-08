//! The IMAP client: connection setup, command execution and reading.
//! Writes live in `write.rs`, change tracking in `changes.rs` / `idle.rs`,
//! flag resynchronization in `resync.rs`.

use std::borrow::Cow;
use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::fmt;
use std::fmt::Write as _;

use imap_proto::types::{
    AttributeValue, Capability, MailboxDatum, MessageSection, ResponseCode, SectionPath,
    UidSetMember,
};
use imap_proto::{Response, Status};
use nail_api::Flags;
use nail_connlog::{Part as LogPart, WireLevel};

use crate::capabilities::Capabilities;
use crate::changes::MailboxChange;
use crate::config::{Credentials, ImapConfig};
use crate::conn::{Connection, Incoming};
use crate::error::{ImapError, Result};
use crate::flags::MessageFlags;
use crate::folder::RemoteFolder;
use crate::sasl::{self, Login, Mechanism};
use crate::transport::{establish, with_timeout};

/// Data items fetched per message. No `ENVELOPE`: the core reads the
/// headers from the message itself, and `ENVELOPE` was the part strict
/// parsers stumbled over (8-bit header values, empty address lists; ADR
/// 0191).
const FETCH_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[])";
/// The same with the mod-sequence, for CONDSTORE servers.
const FETCH_QUERY_MODSEQ: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE MODSEQ BODY.PEEK[])";
/// Data items of a headers-only fetch (sync policy "headers only"): the
/// header block instead of the whole message; `RFC822.SIZE` stays the full
/// size.
const HEADERS_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE BODY.PEEK[HEADER])";
/// The same with the mod-sequence, for CONDSTORE servers.
const HEADERS_QUERY_MODSEQ: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE MODSEQ BODY.PEEK[HEADER])";
/// Maximum number of UIDs per metadata command (`UID FETCH … RFC822.SIZE`),
/// which keeps command lines short for sparse UID sets.
const METADATA_CHUNK: usize = 1000;
/// `LITERAL-` (RFC 7888) allows non-synchronizing literals up to this size.
const LITERAL_MINUS_MAX: usize = 4096;

/// State of a mailbox after `SELECT` / `EXAMINE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    /// UIDVALIDITY: UIDs are only comparable while this stays the same.
    pub uid_validity: u32,
    /// The UID the next arriving message will get (at least).
    pub uid_next: u32,
    /// Number of messages.
    pub exists: u32,
    /// `HIGHESTMODSEQ` (RFC 7162) when the server supports CONDSTORE for
    /// this mailbox; `None` without CONDSTORE or with `NOMODSEQ`.
    pub highest_modseq: Option<u64>,
}

/// What a client remembers about a mailbox between syncs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SyncState {
    /// UIDVALIDITY at the last sync.
    pub uid_validity: u32,
    /// All messages with a UID below this were fetched.
    pub uid_next: u32,
}

/// A UID range `start..=end`; `end == None` means "up to the newest message".
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct UidRange {
    /// First UID (at least 1).
    pub start: u32,
    /// Last UID, inclusive; `None` for `*`.
    pub end: Option<u32>,
}

impl UidRange {
    /// All messages (`1:*`).
    pub const ALL: Self = Self {
        start: 1,
        end: None,
    };

    /// `start:*`.
    pub fn from(start: u32) -> Self {
        Self {
            start: start.max(1),
            end: None,
        }
    }

    /// `start:end` (bounds are ordered and clamped to at least 1).
    pub fn new(start: u32, end: u32) -> Self {
        let (a, b) = (start.min(end).max(1), start.max(end).max(1));
        Self {
            start: a,
            end: Some(b),
        }
    }

    /// Whether `uid` lies in the range.
    pub fn contains(&self, uid: u32) -> bool {
        uid >= self.start && self.end.is_none_or(|end| uid <= end)
    }
}

impl fmt::Display for UidRange {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.end {
            Some(end) => write!(f, "{}:{end}", self.start),
            None => write!(f, "{}:*", self.start),
        }
    }
}

/// One fetched message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMessage {
    /// The message UID.
    pub uid: u32,
    /// System flags.
    pub flags: Flags,
    /// `\Deleted`: marked for removal by the next expunge.
    pub deleted: bool,
    /// Keywords (non-system flags such as `$Forwarded`).
    pub keywords: Vec<String>,
    /// `INTERNALDATE` as RFC 3339, when the server sent a parsable one.
    pub internal_date: Option<String>,
    /// `RFC822.SIZE`.
    pub size: Option<u32>,
    /// Mod-sequence (RFC 7162) on CONDSTORE servers.
    pub modseq: Option<u64>,
    /// The complete raw message (`BODY[]`), or only its header block
    /// (`BODY[HEADER]`, ending with the empty line) when `headers_only`.
    pub raw: Vec<u8>,
    /// Whether `raw` holds only the header block (a headers-only fetch,
    /// see [`crate::BodyFetch`]).
    pub headers_only: bool,
}

/// What [`ImapClient::plan_fetch`] found to fetch in a mailbox: the new
/// UIDs, grouped into batches for [`ImapClient::fetch_batch`].
///
/// A sync fetches and stores one batch at a time and saves
/// [`Self::state_after`] after each, so memory stays bounded by one batch and
/// an interrupted sync resumes after the last stored batch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchPlan {
    /// Mailbox state at the time of the plan.
    pub status: FolderStatus,
    /// `true` when the stored UIDVALIDITY no longer matches: all previously
    /// stored UIDs of this mailbox are invalid and `batches` cover the
    /// complete mailbox.
    pub uid_validity_changed: bool,
    /// UIDs to fetch, ascending, grouped by
    /// [`ImapConfig::fetch_batch`](crate::ImapConfig::fetch_batch).
    pub batches: Vec<Vec<u32>>,
    /// The state to store once every batch is fetched and stored.
    pub final_state: SyncState,
}

impl FetchPlan {
    /// The state to store once batches `0..=index` are fetched and stored:
    /// everything below the batch's last UID + 1 is done.
    pub fn state_after(&self, index: usize) -> SyncState {
        let uid_next = self
            .batches
            .get(index)
            .and_then(|batch| batch.last())
            .map_or(1, |uid| uid.saturating_add(1));
        SyncState {
            uid_validity: self.status.uid_validity,
            uid_next,
        }
    }

    /// Number of messages to fetch.
    pub fn message_count(&self) -> usize {
        self.batches.iter().map(Vec::len).sum()
    }
}

/// Result of [`ImapClient::fetch_new`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct IncrementalFetch {
    /// Mailbox state at the time of the fetch.
    pub status: FolderStatus,
    /// `true` when the stored UIDVALIDITY no longer matches: all previously
    /// stored UIDs of this mailbox are invalid and `messages` holds the
    /// complete mailbox.
    pub uid_validity_changed: bool,
    /// New messages (all messages after a UIDVALIDITY change), in UID order.
    pub messages: Vec<FetchedMessage>,
    /// The state to store for the next incremental fetch.
    pub next_state: SyncState,
}

/// One piece of a command: text, or a literal sent as `{n}` + bytes.
pub(crate) enum Part<'a> {
    Text(Cow<'a, str>),
    Literal(&'a [u8]),
}

/// The tagged `OK` of a command.
#[derive(Debug)]
pub(crate) struct Completion {
    /// The response code, e.g. `APPENDUID` or `COPYUID`.
    pub(crate) code: Option<ResponseCode<'static>>,
    /// The human-readable text (unknown codes such as `[MODIFIED 3]` stay
    /// in it).
    pub(crate) text: String,
}

/// Callback for untagged responses; returns `true` when it consumed the
/// response (otherwise it is recorded as a mailbox change if it is one).
pub(crate) type Handler<'h> = &'h mut (dyn FnMut(&Response<'static>) -> Result<bool> + Send);

/// The mailbox currently opened with `SELECT` or `EXAMINE`.
#[derive(Debug, Clone)]
pub(crate) struct Selected {
    pub(crate) path: String,
    pub(crate) status: FolderStatus,
    pub(crate) read_only: bool,
}

/// Extensions switched on with `ENABLE`.
#[derive(Debug, Clone, Copy, Default)]
pub(crate) struct Enabled {
    pub(crate) condstore: bool,
    pub(crate) qresync: bool,
}

/// What `SELECT` / `EXAMINE` with QRESYNC reported besides the status.
#[derive(Debug, Default)]
pub(crate) struct OpenResult {
    pub(crate) status: Option<FolderStatus>,
    /// `VANISHED (EARLIER)` UID ranges.
    pub(crate) vanished: Vec<std::ops::RangeInclusive<u32>>,
    /// Flag updates (`FETCH` responses).
    pub(crate) fetched: Vec<crate::changes::FlagUpdate>,
}

/// Parameters of a QRESYNC `SELECT` / `EXAMINE` (RFC 7162 section 3.2.5).
#[derive(Debug, Clone)]
pub(crate) struct QresyncParams {
    pub(crate) uid_validity: u32,
    pub(crate) modseq: u64,
    /// Known UIDs as an IMAP set, when short enough to send.
    pub(crate) known_uids: Option<String>,
}

/// What one `UID FETCH` returned ([`ImapClient::fetch_uids_report`]).
#[derive(Debug, Default)]
pub(crate) struct FetchedUids {
    /// The messages, in UID order.
    pub(crate) messages: Vec<FetchedMessage>,
    /// UIDs that still exist but came back without a usable answer.
    pub(crate) withheld: Vec<u32>,
    /// UIDs whose body was larger than Nail reads.
    pub(crate) too_large: Vec<u32>,
}

/// A logged-in IMAP connection.
///
/// After a transient error (connection lost, timeout) the connection is
/// unusable: every further command fails with
/// [`ImapError::ConnectionLost`]; connect again.
pub struct ImapClient {
    pub(crate) conn: Connection,
    pub(crate) capabilities: Capabilities,
    pub(crate) config: ImapConfig,
    pub(crate) selected: Option<Selected>,
    pub(crate) enabled: Enabled,
    pub(crate) server_id: Option<BTreeMap<String, String>>,
    /// Unsolicited mailbox changes not yet taken by the caller.
    pub(crate) changes: Vec<MailboxChange>,
    pub(crate) broken: bool,
    /// The server refused `SELECT` / `EXAMINE` with a `(CONDSTORE)`
    /// parameter once: open mailboxes without it (ADR 0191).
    pub(crate) plain_select: bool,
}

impl fmt::Debug for ImapClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImapClient")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("capabilities", &self.capabilities)
            .field("selected", &self.selected)
            .field("compressed", &self.conn.is_compressed())
            .finish_non_exhaustive()
    }
}

impl ImapClient {
    /// Connects, secures the connection as configured and logs in, then
    /// switches on `COMPRESS=DEFLATE` (when offered and configured), sends
    /// `ID` (when supported and configured) and enables QRESYNC or
    /// CONDSTORE (when offered).
    pub async fn connect(config: &ImapConfig, credentials: &Credentials) -> Result<Self> {
        let (stream, preauth_caps, tap) = establish(config).await?;
        // The password / token never reaches the connection log, also not
        // inside a server's error text.
        tap.add_secret(credentials.secret.expose());
        let login = match sasl::choose_login(config.auth, credentials, &preauth_caps) {
            Ok(login) => login,
            Err(err) => {
                tap.close(Some(&err.to_string()));
                return Err(err);
            }
        };
        let mut client = Self {
            conn: Connection::new(stream, config.timeout, tap),
            capabilities: preauth_caps,
            config: config.clone(),
            selected: None,
            enabled: Enabled::default(),
            server_id: None,
            changes: Vec::new(),
            broken: false,
            plain_select: false,
        };
        match client.log_in(login, credentials).await {
            Ok(()) => Ok(client),
            Err(err) => {
                client.conn.tap().close(Some(&err.to_string()));
                Err(err)
            }
        }
    }

    async fn log_in(&mut self, login: Login, credentials: &Credentials) -> Result<()> {
        let timeout = self.config.timeout;
        let caps = with_timeout(timeout, self.authenticate(login, credentials)).await?;
        match caps {
            Some(caps) => self.capabilities = caps,
            // Capabilities may change after authentication (RFC 3501 6.2).
            None => {
                self.refresh_capabilities().await?;
            }
        }
        self.negotiate_extensions().await
    }

    /// The last command sent on this connection, as the connection log
    /// shows it (redacted; `None` when the log is off).
    pub fn last_command(&self) -> Option<String> {
        self.conn.tap().last_sent()
    }

    /// The last response line received, as the connection log shows it
    /// (redacted; `None` when the log is off) — after a failed command its
    /// tagged `NO` / `BAD`.
    pub fn last_response(&self) -> Option<String> {
        self.conn.tap().last_received()
    }

    /// This connection's id in the connection log.
    pub fn connection_id(&self) -> u32 {
        self.conn.tap().id()
    }

    /// The capabilities advertised after login.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Whether `COMPRESS=DEFLATE` is active.
    pub fn is_compressed(&self) -> bool {
        self.conn.is_compressed()
    }

    /// Whether CONDSTORE is enabled (mod-sequences are reported).
    pub fn condstore_enabled(&self) -> bool {
        self.enabled.condstore
    }

    /// Whether QRESYNC is enabled (expunges arrive as `VANISHED`).
    pub fn qresync_enabled(&self) -> bool {
        self.enabled.qresync
    }

    /// What the server said about itself in reply to `ID`.
    pub fn server_id(&self) -> Option<&BTreeMap<String, String>> {
        self.server_id.as_ref()
    }

    /// The path of the selected (or examined) mailbox.
    pub fn selected_mailbox(&self) -> Option<&str> {
        self.selected.as_ref().map(|s| s.path.as_str())
    }

    /// Whether an earlier error left the connection unusable.
    pub fn is_broken(&self) -> bool {
        self.broken
    }

    /// Logs in; returns the capabilities from a `[CAPABILITY]` response code.
    async fn authenticate(
        &mut self,
        login: Login,
        credentials: &Credentials,
    ) -> Result<Option<Capabilities>> {
        let completion = match login {
            Login::Login => {
                let parts = [
                    Part::Text("LOGIN ".into()),
                    astring(&credentials.username)?,
                    Part::Text(" ".into()),
                    astring(credentials.secret.expose())?,
                ];
                self.execute(&parts, &mut |_| Ok(false)).await
            }
            Login::Sasl(mechanism) => self.authenticate_sasl(mechanism, credentials).await,
        };
        match completion {
            Ok(completion) => Ok(match completion.code {
                Some(ResponseCode::Capabilities(caps)) => Some(capabilities_from(&caps)),
                _ => None,
            }),
            Err(ImapError::No(text) | ImapError::Protocol(text)) => {
                Err(ImapError::AuthFailed(text))
            }
            Err(err) => Err(err),
        }
    }

    async fn authenticate_sasl(
        &mut self,
        mechanism: Mechanism,
        credentials: &Credentials,
    ) -> Result<Completion> {
        let initial = sasl::encode_response(&mechanism.initial_response(
            credentials,
            &self.config.host,
            self.config.port,
        ));
        let tag = self.conn.next_tag();
        let mut initial_sent = self.capabilities.has("SASL-IR");
        let command = if initial_sent {
            format!("{tag} AUTHENTICATE {} {initial}", mechanism.name())
        } else {
            format!("{tag} AUTHENTICATE {}", mechanism.name())
        };
        self.conn.tap().sent(&[LogPart::Text(&command)]);
        self.conn.write(format!("{command}\r\n").as_bytes()).await?;
        let mut server_error = None;
        loop {
            let response = match self.conn.read().await? {
                Incoming::Response(response) => response,
                Incoming::Unparsed => continue,
            };
            match response {
                Response::Continue { information, .. } => {
                    let reply = if initial_sent {
                        // A challenge after our response: the server's error
                        // details (OAuth) or something we cannot answer.
                        server_error = information
                            .as_deref()
                            .map(sasl::describe_challenge)
                            .filter(|d| !d.is_empty());
                        mechanism.challenge_response().to_owned()
                    } else {
                        initial_sent = true;
                        initial.clone()
                    };
                    self.conn.tap().sent(&[LogPart::Text(&reply)]);
                    self.conn.write(format!("{reply}\r\n").as_bytes()).await?;
                }
                Response::Done {
                    tag: done,
                    status,
                    code,
                    information,
                } if done.0 == tag => {
                    let text = response_text(code.as_ref(), information.as_deref());
                    return match status {
                        Status::Ok => Ok(Completion { code, text }),
                        _ => Err(ImapError::AuthFailed(match server_error {
                            Some(details) => format!("{text} ({details})"),
                            None => text,
                        })),
                    };
                }
                _ => {}
            }
        }
    }

    /// `COMPRESS`, `ID` and `ENABLE` after login.
    async fn negotiate_extensions(&mut self) -> Result<()> {
        if self.config.compress && self.capabilities.has("COMPRESS=DEFLATE") {
            match self.command("COMPRESS DEFLATE", |_| Ok(false)).await {
                Ok(_) => self.conn.start_compression()?,
                // A refusal (e.g. COMPRESSIONACTIVE) is not fatal.
                Err(err @ (ImapError::No(_) | ImapError::Protocol(_))) if !self.broken => {
                    tracing::debug!(%err, "COMPRESS refused");
                }
                Err(err) => return Err(err),
            }
        }
        if self.capabilities.has("ID")
            && let Some(id) = self.config.client_id.clone()
        {
            let mut params = String::new();
            for (key, value) in &id {
                if !params.is_empty() {
                    params.push(' ');
                }
                params.push_str(&quote(key)?);
                params.push(' ');
                params.push_str(&quote(value)?);
            }
            let mut server = None;
            let result = self
                .command(&format!("ID ({params})"), |response| {
                    if let Response::Id(map) = response {
                        server = Some(
                            map.iter()
                                .flatten()
                                .map(|(k, v)| (k.to_string(), v.to_string()))
                                .collect(),
                        );
                        return Ok(true);
                    }
                    Ok(false)
                })
                .await;
            match result {
                Ok(_) => self.server_id = server,
                Err(err @ (ImapError::No(_) | ImapError::Protocol(_))) if !self.broken => {
                    tracing::debug!(%err, "ID refused");
                }
                Err(err) => return Err(err),
            }
        }
        if self.capabilities.has("ENABLE") {
            let wanted = if self.capabilities.has("QRESYNC") {
                Some("QRESYNC")
            } else if self.capabilities.has("CONDSTORE") {
                Some("CONDSTORE")
            } else {
                None
            };
            if let Some(wanted) = wanted {
                let mut enabled = Vec::new();
                let result = self
                    .command(&format!("ENABLE {wanted}"), |response| {
                        // `* ENABLED ...` parses like a capability list.
                        if let Response::Capabilities(caps) = response {
                            enabled.extend(caps.iter().map(capability_name));
                            return Ok(true);
                        }
                        Ok(false)
                    })
                    .await;
                match result {
                    Ok(_) => {
                        let has = |name: &str| enabled.iter().any(|e| e.eq_ignore_ascii_case(name));
                        self.enabled.qresync = has("QRESYNC");
                        self.enabled.condstore = self.enabled.qresync || has("CONDSTORE");
                    }
                    Err(err @ (ImapError::No(_) | ImapError::Protocol(_))) if !self.broken => {
                        tracing::debug!(%err, "ENABLE refused");
                    }
                    Err(err) => return Err(err),
                }
            }
        }
        Ok(())
    }

    /// Re-reads the capabilities from the server.
    pub async fn refresh_capabilities(&mut self) -> Result<&Capabilities> {
        let mut atoms = Vec::new();
        self.command("CAPABILITY", |response| {
            if let Response::Capabilities(caps) = response {
                atoms.extend(caps.iter().map(capability_name));
                return Ok(true);
            }
            Ok(false)
        })
        .await?;
        if atoms.is_empty() {
            return Err(ImapError::Protocol("no CAPABILITY response".to_owned()));
        }
        self.capabilities = Capabilities::from_atoms(atoms);
        Ok(&self.capabilities)
    }

    /// Lists all mailboxes (`LIST "" "*"`) with their roles.
    pub async fn list_folders(&mut self) -> Result<Vec<RemoteFolder>> {
        let mut folders = Vec::new();
        self.command("LIST \"\" \"*\"", |response| {
            if let Response::MailboxData(MailboxDatum::List {
                name_attributes,
                delimiter,
                name,
            }) = response
            {
                folders.push(RemoteFolder::from_list(
                    name,
                    delimiter.as_deref(),
                    name_attributes,
                ));
                return Ok(true);
            }
            Ok(false)
        })
        .await?;
        Ok(folders)
    }

    /// Opens a mailbox read-only (`EXAMINE`) and returns its state.
    pub async fn examine(&mut self, path: &str) -> Result<FolderStatus> {
        self.open(path, true, None)
            .await?
            .status
            .ok_or_else(|| ImapError::Protocol(format!("no status for {path}")))
    }

    /// Opens a mailbox read-write (`SELECT`) and returns its state. The
    /// write methods select their mailbox themselves.
    pub async fn select(&mut self, path: &str) -> Result<FolderStatus> {
        self.open(path, false, None)
            .await?
            .status
            .ok_or_else(|| ImapError::Protocol(format!("no status for {path}")))
    }

    /// Makes `path` the selected mailbox read-write, reusing the current
    /// selection when it fits, and returns its state (its UIDVALIDITY tells
    /// whether UIDs recorded earlier still name the same messages).
    pub async fn select_for_write(&mut self, path: &str) -> Result<FolderStatus> {
        self.ensure_selected(path, true).await?;
        self.selected
            .as_ref()
            .map(|s| s.status)
            .ok_or_else(|| ImapError::Protocol(format!("no status for {path}")))
    }

    /// The UIDs of the messages in `path` whose `Message-ID` header is
    /// `message_id` (angle brackets optional), ascending (`UID SEARCH
    /// HEADER`). Selects `path` read-write, like the write methods.
    pub async fn find_by_message_id(&mut self, path: &str, message_id: &str) -> Result<Vec<u32>> {
        let id = message_id
            .trim()
            .trim_start_matches('<')
            .trim_end_matches('>');
        if id.is_empty() {
            return Ok(Vec::new());
        }
        // With the brackets, the substring match of SEARCH HEADER only
        // matches this exact id.
        let wanted = quote(&format!("<{id}>"))?;
        self.ensure_selected(path, true).await?;
        self.search(&format!("HEADER Message-ID {wanted}"), |_| true)
            .await
    }

    /// Makes `path` the selected mailbox, read-write when `writable`;
    /// reuses the current selection when it fits.
    pub(crate) async fn ensure_selected(&mut self, path: &str, writable: bool) -> Result<()> {
        let fits = self
            .selected
            .as_ref()
            .is_some_and(|s| s.path == path && (!writable || !s.read_only));
        if !fits {
            self.open(path, !writable, None).await?;
        }
        Ok(())
    }

    /// `SELECT` / `EXAMINE`, with `(CONDSTORE)` or `(QRESYNC ...)`.
    pub(crate) async fn open(
        &mut self,
        path: &str,
        read_only: bool,
        qresync: Option<QresyncParams>,
    ) -> Result<OpenResult> {
        self.selected = None;
        self.changes.clear();
        let verb = if read_only { "EXAMINE" } else { "SELECT" };
        let mut command = format!("{verb} {}", quote(path)?);
        match &qresync {
            Some(params) if self.enabled.qresync => {
                let _ = write!(
                    command,
                    " (QRESYNC ({} {}",
                    params.uid_validity, params.modseq
                );
                if let Some(known) = &params.known_uids {
                    command.push(' ');
                    command.push_str(known);
                }
                command.push_str("))");
            }
            _ if self.capabilities.has("CONDSTORE") && !self.plain_select => {
                command.push_str(" (CONDSTORE)");
            }
            _ => {}
        }
        let with_parameter = command.ends_with(')');
        let (mut exists, mut uid_validity, mut uid_next, mut modseq, mut nomodseq) =
            (None, None, None, None, false);
        let mut result = OpenResult::default();
        let opened = self
            .command(&command, |response| {
                match response {
                    Response::MailboxData(MailboxDatum::Exists(n)) => exists = Some(*n),
                    Response::Data {
                        code: Some(ResponseCode::UidValidity(v)),
                        ..
                    } => uid_validity = Some(*v),
                    Response::Data {
                        code: Some(ResponseCode::UidNext(n)),
                        ..
                    } => uid_next = Some(*n),
                    Response::Data {
                        code: Some(ResponseCode::HighestModSeq(n)),
                        ..
                    } => modseq = Some(*n),
                    Response::Data {
                        code: None,
                        information: Some(text),
                        ..
                    } if text.to_ascii_uppercase().starts_with("[NOMODSEQ]") => nomodseq = true,
                    Response::Vanished {
                        earlier: true,
                        uids,
                    } => {
                        result.vanished.extend(uids.iter().cloned());
                    }
                    Response::Fetch(_, attributes) => {
                        if let Some(update) = crate::changes::flag_update(attributes) {
                            result.fetched.push(update);
                        }
                    }
                    _ => return Ok(false),
                }
                Ok(true)
            })
            .await;
        match opened {
            // Some servers advertise CONDSTORE but refuse the SELECT
            // parameter: open without it from now on (ADR 0191).
            Err(err @ (ImapError::No(_) | ImapError::Protocol(_)))
                if with_parameter && qresync.is_none() && !self.broken =>
            {
                tracing::info!(%err, "SELECT (CONDSTORE) refused; opening without it");
                self.conn.tap().note(
                    WireLevel::Warning,
                    "the server refused (CONDSTORE); opening without it",
                );
                self.plain_select = true;
                return Box::pin(self.open(path, read_only, None)).await;
            }
            other => {
                other?;
            }
        }
        let uid_validity = uid_validity
            .ok_or_else(|| ImapError::Protocol(format!("no UIDVALIDITY for {path}")))?;
        let exists = exists.ok_or_else(|| ImapError::Protocol(format!("no EXISTS for {path}")))?;
        let mut status = FolderStatus {
            uid_validity,
            uid_next: 1,
            exists,
            highest_modseq: modseq.filter(|_| !nomodseq),
        };
        self.selected = Some(Selected {
            path: path.to_owned(),
            status,
            read_only,
        });
        status.uid_next = match uid_next {
            Some(uid_next) => uid_next,
            // Servers must send UIDNEXT, but some old ones do not.
            None => self
                .search_uids(UidRange::ALL)
                .await?
                .last()
                .map_or(1, |uid| uid + 1),
        };
        if let Some(selected) = &mut self.selected {
            selected.status = status;
        }
        result.status = Some(status);
        Ok(result)
    }

    /// Fetches all messages of the examined mailbox whose UID lies in
    /// `range` (raw bytes, flags), in UID order, in batches.
    /// Call [`Self::examine`] first.
    ///
    /// Holds every message in memory; to sync a whole mailbox use
    /// [`Self::plan_fetch`] and [`Self::fetch_batch`] instead.
    pub async fn fetch_range(&mut self, range: UidRange) -> Result<Vec<FetchedMessage>> {
        if self.selected.is_none() {
            return Err(ImapError::Protocol("no mailbox examined".to_owned()));
        }
        let uids = self.search_uids(range).await?;
        let mut messages = Vec::with_capacity(uids.len());
        for batch in uids.chunks(self.config.fetch_batch.max_messages.max(1)) {
            messages.extend(self.fetch_batch(batch).await?);
        }
        Ok(messages)
    }

    /// Fetches the messages with these UIDs from the examined mailbox in one
    /// `UID FETCH`, in UID order. UIDs expunged meanwhile are skipped; a
    /// message the server withholds is an error.
    ///
    /// Bodies can be large, so the command has no overall deadline: it fails
    /// only when the server stays silent for the configured timeout.
    pub async fn fetch_batch(&mut self, uids: &[u32]) -> Result<Vec<FetchedMessage>> {
        self.fetch_uids(uids, false).await
    }

    /// [`Self::fetch_batch`] with the header block only (`headers_only`)
    /// or the whole message. A message the server withholds (or that came
    /// back unreadable or too large) is an error.
    pub(crate) async fn fetch_uids(
        &mut self,
        uids: &[u32],
        headers_only: bool,
    ) -> Result<Vec<FetchedMessage>> {
        let fetched = self.fetch_uids_report(uids, headers_only).await?;
        if let Some(uid) = fetched.too_large.first() {
            return Err(ImapError::TooLarge(format!("message {uid}")));
        }
        if !fetched.withheld.is_empty() {
            return Err(ImapError::Protocol(format!(
                "server did not return messages {}",
                uid_set(&fetched.withheld)
            )));
        }
        Ok(fetched.messages)
    }

    /// One `UID FETCH` of `uids`: the messages that came back, the UIDs the
    /// server withheld although they still exist (no answer, or one that
    /// could not be read), and those larger than Nail reads.
    pub(crate) async fn fetch_uids_report(
        &mut self,
        uids: &[u32],
        headers_only: bool,
    ) -> Result<FetchedUids> {
        if self.selected.is_none() {
            return Err(ImapError::Protocol("no mailbox examined".to_owned()));
        }
        let mut wanted = uids.to_vec();
        wanted.sort_unstable();
        wanted.dedup();
        if wanted.is_empty() {
            return Ok(FetchedUids::default());
        }
        let query = match (headers_only, self.capabilities.has("CONDSTORE")) {
            (false, true) => FETCH_QUERY_MODSEQ,
            (false, false) => FETCH_QUERY,
            (true, true) => HEADERS_QUERY_MODSEQ,
            (true, false) => HEADERS_QUERY,
        };
        let mut fetched = Vec::with_capacity(wanted.len());
        let command = format!("UID FETCH {} {query}", uid_set(&wanted));
        let mut on = |response: &Response<'static>| {
            if let Response::Fetch(_, attributes) = response {
                // Unsolicited FETCH responses (flag updates) for other
                // messages may be interleaved; keep only what was asked.
                if let Some(message) = convert_fetch(attributes)
                    .filter(|m| m.headers_only == headers_only)
                    .filter(|m| wanted.binary_search(&m.uid).is_ok())
                {
                    fetched.push(message);
                    return Ok(true);
                }
            }
            Ok(false)
        };
        self.run_parts(&[Part::Text(command.into())], &mut on, false)
            .await?;
        fetched.sort_by_key(|m| m.uid);
        fetched.dedup_by_key(|m| m.uid);
        // An oversized literal arrives empty (see `conn.rs`).
        let (too_large, fetched): (Vec<FetchedMessage>, Vec<FetchedMessage>) = fetched
            .into_iter()
            .partition(|m| m.raw.is_empty() && m.size.is_some_and(|s| s > 0));
        let withheld = self.still_present(&wanted, &fetched, &too_large).await?;
        Ok(FetchedUids {
            messages: fetched,
            withheld,
            too_large: too_large.iter().map(|m| m.uid).collect(),
        })
    }

    /// Examines `path` and plans fetching what is new since `known` (a
    /// previous [`FetchPlan::final_state`] or [`FetchPlan::state_after`]).
    /// Without `known`, or when the server's UIDVALIDITY changed, the whole
    /// mailbox is planned. Batches are sized by `RFC822.SIZE`.
    pub async fn plan_fetch(&mut self, path: &str, known: Option<SyncState>) -> Result<FetchPlan> {
        let status = self.examine(path).await?;
        let uid_validity_changed = known.is_some_and(|k| k.uid_validity != status.uid_validity);
        let known = known.filter(|_| !uid_validity_changed);
        let range = known.map_or(UidRange::ALL, |k| UidRange::from(k.uid_next));
        let nothing_new = known.is_some_and(|k| status.uid_next <= k.uid_next);
        let uids = if nothing_new {
            Vec::new()
        } else {
            self.search_uids(range).await?
        };
        // Sizes only shape the batches: a server that refuses the metadata
        // fetch (long UID sets, ADR 0191) gets batches by count instead.
        let sizes = match self.fetch_sizes(&uids).await {
            Ok(sizes) => sizes,
            Err(err @ (ImapError::No(_) | ImapError::Protocol(_))) if !self.broken => {
                tracing::info!(%err, "message sizes refused; batching by count");
                HashMap::new()
            }
            Err(err) => return Err(err),
        };
        let batches = batch_uids(&uids, &sizes, self.config.fetch_batch);

        let planned_next = uids.last().map_or(0, |uid| uid.saturating_add(1));
        let known_next = known.map_or(1, |k| k.uid_next);
        Ok(FetchPlan {
            status,
            uid_validity_changed,
            batches,
            final_state: SyncState {
                uid_validity: status.uid_validity,
                uid_next: status.uid_next.max(planned_next).max(known_next),
            },
        })
    }

    /// `RFC822.SIZE` of the messages with these UIDs in the examined mailbox.
    /// Messages the server does not report are missing from the map.
    async fn fetch_sizes(&mut self, uids: &[u32]) -> Result<HashMap<u32, u32>> {
        let mut sizes = HashMap::with_capacity(uids.len());
        for chunk in uids.chunks(METADATA_CHUNK) {
            self.command(
                &format!("UID FETCH {} (UID RFC822.SIZE)", uid_set(chunk)),
                |response| {
                    if let Response::Fetch(_, attributes) = response {
                        let (mut uid, mut size) = (None, None);
                        for attribute in attributes {
                            match attribute {
                                AttributeValue::Uid(v) => uid = Some(*v),
                                AttributeValue::Rfc822Size(v) => size = Some(*v),
                                _ => {}
                            }
                        }
                        if let (Some(uid), Some(size)) = (uid, size) {
                            sizes.insert(uid, size);
                            return Ok(true);
                        }
                    }
                    Ok(false)
                },
            )
            .await?;
        }
        Ok(sizes)
    }

    /// The requested UIDs without a (usable) response that still exist: a
    /// UID without a response is fine only if the message was expunged
    /// meanwhile; otherwise the server failed us silently.
    async fn still_present(
        &mut self,
        requested: &[u32],
        fetched: &[FetchedMessage],
        too_large: &[FetchedMessage],
    ) -> Result<Vec<u32>> {
        let got: BTreeSet<u32> = fetched.iter().chain(too_large).map(|m| m.uid).collect();
        let missing: Vec<u32> = requested
            .iter()
            .copied()
            .filter(|uid| !got.contains(uid))
            .collect();
        if missing.is_empty() {
            return Ok(Vec::new());
        }
        let mut still_there = Vec::new();
        self.command(
            &format!("UID SEARCH UID {}", uid_set(&missing)),
            |response| {
                if let Response::MailboxData(MailboxDatum::Search(uids)) = response {
                    still_there.extend(
                        uids.iter()
                            .copied()
                            .filter(|uid| missing.binary_search(uid).is_ok()),
                    );
                    return Ok(true);
                }
                Ok(false)
            },
        )
        .await?;
        still_there.sort_unstable();
        still_there.dedup();
        Ok(still_there)
    }

    /// Examines `path` and fetches what is new since `known` (a previous
    /// [`IncrementalFetch::next_state`]). Without `known`, or when the
    /// server's UIDVALIDITY changed, the whole mailbox is fetched.
    ///
    /// Holds every new message in memory; a sync of possibly large mailboxes
    /// uses [`Self::plan_fetch`] and [`Self::fetch_batch`] and stores each
    /// batch as it arrives.
    pub async fn fetch_new(
        &mut self,
        path: &str,
        known: Option<SyncState>,
    ) -> Result<IncrementalFetch> {
        let plan = self.plan_fetch(path, known).await?;
        let mut messages = Vec::with_capacity(plan.message_count());
        for batch in &plan.batches {
            messages.extend(self.fetch_batch(batch).await?);
        }
        Ok(IncrementalFetch {
            status: plan.status,
            uid_validity_changed: plan.uid_validity_changed,
            next_state: plan.final_state,
            messages,
        })
    }

    /// Sends `NOOP` (keep-alive / poll for changes; see
    /// [`Self::take_changes`] and [`Self::poll`]).
    pub async fn noop(&mut self) -> Result<()> {
        self.command("NOOP", |_| Ok(false)).await.map(drop)
    }

    /// Logs out and closes the connection. The server closes its side
    /// right after `BYE`; a connection lost after that is not an error.
    pub async fn logout(mut self) -> Result<()> {
        match self.command("LOGOUT", |_| Ok(false)).await {
            Ok(_) | Err(ImapError::ConnectionLost) => Ok(()),
            Err(err) => Err(err),
        }
    }

    /// UIDs in `range` in the selected mailbox, ascending. Filters the
    /// result, because `n:*` always matches the highest UID even when it is
    /// below `n` (RFC 3501 6.4.8).
    pub(crate) async fn search_uids(&mut self, range: UidRange) -> Result<Vec<u32>> {
        self.search(&format!("UID {range}"), |uid| range.contains(uid))
            .await
    }

    /// `UID SEARCH <criteria>`, ascending, keeping UIDs that pass `keep`.
    pub(crate) async fn search(
        &mut self,
        criteria: &str,
        keep: impl Fn(u32) -> bool + Send + Sync,
    ) -> Result<Vec<u32>> {
        let mut uids = Vec::new();
        self.command(&format!("UID SEARCH {criteria}"), |response| {
            if let Response::MailboxData(MailboxDatum::Search(found)) = response {
                uids.extend(found.iter().copied().filter(|uid| keep(*uid)));
                return Ok(true);
            }
            Ok(false)
        })
        .await?;
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// Runs a simple command with the configured deadline.
    pub(crate) async fn command(
        &mut self,
        command: &str,
        mut on: impl FnMut(&Response<'static>) -> Result<bool> + Send,
    ) -> Result<Completion> {
        self.run_parts(&[Part::Text(Cow::Borrowed(command))], &mut on, true)
            .await
    }

    /// Runs a command. With `deadline`, the whole command must finish within
    /// the configured timeout; without, only the read idle timeout applies
    /// (for commands that transfer message bodies).
    ///
    /// Transient failures mark the connection as broken.
    pub(crate) async fn run_parts(
        &mut self,
        parts: &[Part<'_>],
        on: Handler<'_>,
        deadline: bool,
    ) -> Result<Completion> {
        if self.broken {
            return Err(ImapError::ConnectionLost);
        }
        let timeout = self.config.timeout;
        let result = if deadline {
            with_timeout(timeout, self.execute(parts, on)).await
        } else {
            self.execute(parts, on).await
        };
        let result = result.map_err(|err| match err {
            ImapError::Io(io) if io.kind() == std::io::ErrorKind::TimedOut => {
                ImapError::Timeout(timeout)
            }
            other => other,
        });
        if let Err(err) = &result {
            if err.is_transient() {
                self.broken = true;
                self.conn.tap().note(WireLevel::Error, &err.to_string());
            } else if !matches!(err, ImapError::No(_)) {
                // `BAD` / unreadable answers: the tagged line is logged
                // already; this says what Nail made of it.
                self.conn.tap().note(WireLevel::Warning, &err.to_string());
            }
        }
        result
    }

    /// Sends a command (with literals) and reads until its tagged
    /// completion, which must be `OK`. Untagged responses go to `on` and,
    /// unless consumed, into the change log.
    ///
    /// This runner turns a closed connection into
    /// [`ImapError::ConnectionLost`] and `NO`/`BAD` into errors, so a
    /// truncated result is never mistaken for a complete one.
    async fn execute(&mut self, parts: &[Part<'_>], on: Handler<'_>) -> Result<Completion> {
        let tag = self.conn.next_tag();
        if self.conn.tap().is_on() {
            let head = format!("{tag} ");
            let mut logged = vec![LogPart::Text(&head)];
            for part in parts {
                logged.push(match part {
                    Part::Text(text) => LogPart::Text(text),
                    Part::Literal(bytes) => LogPart::Bytes(bytes.len()),
                });
            }
            self.conn.tap().sent(&logged);
        }
        let literal_plus = self.capabilities.has("LITERAL+");
        let literal_minus = self.capabilities.has("LITERAL-");
        let mut line = format!("{tag} ");
        for part in parts {
            match part {
                Part::Text(text) => line.push_str(text),
                Part::Literal(bytes) => {
                    let non_sync =
                        literal_plus || (literal_minus && bytes.len() <= LITERAL_MINUS_MAX);
                    let _ = write!(
                        line,
                        "{{{}{}}}\r\n",
                        bytes.len(),
                        if non_sync { "+" } else { "" }
                    );
                    self.conn.write(line.as_bytes()).await?;
                    line.clear();
                    if !non_sync {
                        self.wait_for_continuation(&tag, on).await?;
                    }
                    self.conn.write(bytes).await?;
                }
            }
        }
        line.push_str("\r\n");
        self.conn.write(line.as_bytes()).await?;
        self.finish(&tag, on).await
    }

    /// Waits for `+` after a synchronizing literal; the server may refuse
    /// the command instead (e.g. `NO [TOOBIG]`).
    async fn wait_for_continuation(&mut self, tag: &str, on: Handler<'_>) -> Result<()> {
        match self.next_tagged(tag, on).await? {
            Tagged::Continue(_) => Ok(()),
            Tagged::Done(Status::Ok, _, text) => Err(ImapError::Protocol(format!(
                "command completed before its literal: {text}"
            ))),
            Tagged::Done(status, _, text) => Err(refusal(status, text)),
        }
    }

    /// Reads until the tagged completion of `tag`.
    pub(crate) async fn finish(&mut self, tag: &str, on: Handler<'_>) -> Result<Completion> {
        match self.next_tagged(tag, on).await? {
            Tagged::Continue(information) => Err(ImapError::Protocol(format!(
                "unexpected continuation request: {information}"
            ))),
            Tagged::Done(Status::Ok, code, text) => Ok(Completion { code, text }),
            Tagged::Done(status, _, text) => Err(refusal(status, text)),
        }
    }

    /// Reads up to the next continuation request or the tagged completion
    /// of `tag`, dispatching the untagged responses before it.
    async fn next_tagged(&mut self, tag: &str, on: Handler<'_>) -> Result<Tagged> {
        loop {
            match self.conn.read().await? {
                Incoming::Response(Response::Continue { information, .. }) => {
                    return Ok(Tagged::Continue(
                        information.as_deref().unwrap_or_default().to_owned(),
                    ));
                }
                Incoming::Response(response) => match completion(response, tag) {
                    Ok((status, code, text)) => return Ok(Tagged::Done(status, code, text)),
                    Err(response) => self.dispatch(&response, on)?,
                },
                Incoming::Unparsed => {}
            }
        }
    }

    /// Passes an untagged response to the handler and records it as a
    /// mailbox change when the handler did not consume it.
    pub(crate) fn dispatch(&mut self, response: &Response<'static>, on: Handler<'_>) -> Result<()> {
        if !on(response)? {
            self.observe(response);
        }
        Ok(())
    }
}

/// Groups ascending `uids` into batches of at most `limits.max_messages`
/// messages and `limits.max_bytes` bytes (by `sizes`; unknown sizes count as
/// zero). A message larger than the byte limit gets a batch of its own.
fn batch_uids(
    uids: &[u32],
    sizes: &HashMap<u32, u32>,
    limits: crate::FetchBatchLimits,
) -> Vec<Vec<u32>> {
    let max_messages = limits.max_messages.max(1);
    let mut batches = Vec::new();
    let mut current: Vec<u32> = Vec::new();
    let mut bytes = 0u64;
    for &uid in uids {
        let size = u64::from(sizes.get(&uid).copied().unwrap_or(0));
        if !current.is_empty()
            && (current.len() >= max_messages || bytes.saturating_add(size) > limits.max_bytes)
        {
            batches.push(std::mem::take(&mut current));
            bytes = 0;
        }
        current.push(uid);
        bytes = bytes.saturating_add(size);
    }
    if !current.is_empty() {
        batches.push(current);
    }
    batches
}

/// The text of a tagged or untagged status response, with the response
/// codes `imap-proto` parsed put back in front (`[TRYCREATE] ...`), so
/// error mapping can see them.
/// What ends a command's wait for the server: a continuation request (`+`,
/// with its text) or the tagged completion (status, code, text).
enum Tagged {
    Continue(String),
    Done(Status, Option<ResponseCode<'static>>, String),
}

/// The status, response code and text of the tagged completion of `tag`;
/// any other response comes back as it is.
pub(crate) fn completion(
    response: Response<'static>,
    tag: &str,
) -> std::result::Result<(Status, Option<ResponseCode<'static>>, String), Response<'static>> {
    match response {
        Response::Done {
            tag: done,
            status,
            code,
            information,
        } if done.0 == tag => {
            let text = response_text(code.as_ref(), information.as_deref());
            Ok((status, code, text))
        }
        other => Err(other),
    }
}

/// The error of a tagged completion that is not `OK`.
pub(crate) fn refusal(status: Status, text: String) -> ImapError {
    match status {
        Status::No => ImapError::No(text),
        other => ImapError::Protocol(format!("{other:?} {text}")),
    }
}

pub(crate) fn response_text(code: Option<&ResponseCode<'_>>, information: Option<&str>) -> String {
    let prefix = match code {
        Some(ResponseCode::TryCreate) => "[TRYCREATE] ",
        Some(ResponseCode::Alert) => "[ALERT] ",
        Some(ResponseCode::ReadOnly) => "[READ-ONLY] ",
        Some(ResponseCode::Parse) => "[PARSE] ",
        Some(ResponseCode::UidNotSticky) => "[UIDNOTSTICKY] ",
        _ => "",
    };
    format!("{prefix}{}", information.unwrap_or_default())
}

fn capability_name(capability: &Capability<'_>) -> String {
    match capability {
        Capability::Imap4rev1 => "IMAP4REV1".to_owned(),
        Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
        Capability::Atom(atom) => atom.to_string(),
    }
}

fn capabilities_from(caps: &[Capability<'_>]) -> Capabilities {
    Capabilities::from_atoms(caps.iter().map(capability_name))
}

/// Quotes a string (mailbox name, `ID` value) as an IMAP quoted string.
pub(crate) fn quote(value: &str) -> Result<String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(ImapError::InvalidArgument(
            "value contains control characters".to_owned(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

/// A user name or password: quoted when it is printable ASCII, otherwise a
/// literal (8-bit passwords).
fn astring(value: &str) -> Result<Part<'_>> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(ImapError::InvalidArgument(
            "credentials contain line breaks or NUL".to_owned(),
        ));
    }
    if value.bytes().all(|b| (0x20..0x7f).contains(&b)) {
        Ok(Part::Text(Cow::Owned(quote(value)?)))
    } else {
        Ok(Part::Literal(value.as_bytes()))
    }
}

/// Compresses sorted UIDs into an IMAP set: `1:3,7,9:10`.
pub(crate) use nail_net::wire::sequence_set as uid_set;

/// Sorted, deduplicated copy of `uids`; fails when empty or containing 0.
pub(crate) fn sorted_uids(uids: &[u32]) -> Result<Vec<u32>> {
    let mut sorted = uids.to_vec();
    sorted.sort_unstable();
    sorted.dedup();
    if sorted.is_empty() || sorted[0] == 0 {
        return Err(ImapError::InvalidArgument(
            "a non-empty set of non-zero UIDs is required".to_owned(),
        ));
    }
    Ok(sorted)
}

/// Expands a `COPYUID` / `APPENDUID` UID set, in order.
pub(crate) fn expand_uid_set(members: &[UidSetMember]) -> Vec<u32> {
    let mut out = Vec::new();
    for member in members {
        match member {
            UidSetMember::Uid(uid) => out.push(*uid),
            UidSetMember::UidRange(range) => {
                let (a, b) = (*range.start(), *range.end());
                if a <= b {
                    out.extend(a..=b);
                } else {
                    out.extend((b..=a).rev());
                }
            }
        }
    }
    out
}

/// Converts the attributes of one `FETCH` response; `None` without UID or body.
fn convert_fetch(attributes: &[AttributeValue<'_>]) -> Option<FetchedMessage> {
    let mut uid = None;
    let mut flags = MessageFlags::default();
    let mut internal_date = None;
    let mut size = None;
    let mut modseq = None;
    let mut raw = None;
    let mut headers_only = false;
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::Flags(values) => {
                flags = MessageFlags::from_imap(values.iter().map(AsRef::<str>::as_ref));
            }
            AttributeValue::InternalDate(value) => internal_date = internal_date_to_rfc3339(value),
            AttributeValue::Rfc822Size(value) => size = Some(*value),
            AttributeValue::ModSeq(value) => modseq = Some(*value),
            AttributeValue::BodySection {
                section: None,
                data: Some(data),
                ..
            }
            | AttributeValue::Rfc822(Some(data)) => raw = Some(data.to_vec()),
            AttributeValue::BodySection {
                section: Some(SectionPath::Full(MessageSection::Header)),
                data: Some(data),
                ..
            }
            | AttributeValue::Rfc822Header(Some(data)) => {
                raw = Some(data.to_vec());
                headers_only = true;
            }
            _ => {}
        }
    }
    let uid = uid?;
    let Some(raw) = raw else {
        // A flags-only (unsolicited) update; the real response carries the body.
        tracing::debug!(uid, "FETCH response without body");
        return None;
    };
    Some(FetchedMessage {
        uid,
        flags: flags.flags,
        deleted: flags.deleted,
        keywords: flags.keywords,
        internal_date,
        size,
        modseq,
        raw,
        headers_only,
    })
}

const MONTHS: [&str; 12] = [
    "jan", "feb", "mar", "apr", "may", "jun", "jul", "aug", "sep", "oct", "nov", "dec",
];

/// Converts an IMAP date-time (`28-Sep-2026 08:15:00 +0200`, day may be
/// space-padded) to RFC 3339.
fn internal_date_to_rfc3339(value: &str) -> Option<String> {
    let (date, rest) = value.trim().split_once(' ')?;
    let (time, zone) = rest.trim().split_once(' ')?;
    let mut date_parts = date.split('-');
    let day: u8 = date_parts.next()?.trim().parse().ok()?;
    let month_name = date_parts.next()?.to_ascii_lowercase();
    let month = MONTHS.iter().position(|m| *m == month_name)? + 1;
    let year: u16 = date_parts.next()?.parse().ok()?;
    let mut time_parts = time.split(':').map(|p| p.parse::<u8>().ok());
    let (hour, minute, second) = (
        time_parts.next()??,
        time_parts.next()??,
        time_parts.next()??,
    );
    let (sign, digits) = zone.split_at_checked(1)?;
    if !matches!(sign, "+" | "-")
        || digits.len() != 4
        || !digits.bytes().all(|b| b.is_ascii_digit())
    {
        return None;
    }
    if !(1..=31).contains(&day) || hour > 23 || minute > 59 || second > 60 {
        return None;
    }
    let offset = if digits == "0000" {
        "Z".to_owned()
    } else {
        format!("{sign}{}:{}", &digits[..2], &digits[2..])
    };
    Some(format!(
        "{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}{offset}"
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::FetchBatchLimits;

    #[test]
    fn batches_respect_count_and_size_limits() {
        let limits = FetchBatchLimits {
            max_messages: 3,
            max_bytes: 100,
        };
        let sizes: HashMap<u32, u32> = [
            (1, 10),
            (2, 10),
            (3, 10),
            (4, 10),
            (5, 90),
            (6, 500),
            (7, 20),
        ]
        .into();
        assert_eq!(
            batch_uids(&[1, 2, 3, 4, 5, 6, 7, 8], &sizes, limits),
            vec![vec![1, 2, 3], vec![4, 5], vec![6], vec![7, 8]]
        );
        assert_eq!(batch_uids(&[], &sizes, limits).len(), 0);
        // Zero limits still make progress, one message per batch.
        let zero = FetchBatchLimits {
            max_messages: 0,
            max_bytes: 0,
        };
        assert_eq!(batch_uids(&[1, 2], &sizes, zero), vec![vec![1], vec![2]]);
    }

    #[test]
    fn plan_checkpoints() {
        let plan = FetchPlan {
            status: FolderStatus {
                uid_validity: 7,
                uid_next: 20,
                exists: 3,
                highest_modseq: None,
            },
            uid_validity_changed: false,
            batches: vec![vec![3, 5], vec![9]],
            final_state: SyncState {
                uid_validity: 7,
                uid_next: 20,
            },
        };
        assert_eq!(plan.message_count(), 3);
        assert_eq!(plan.state_after(0).uid_next, 6);
        assert_eq!(plan.state_after(1).uid_next, 10);
        assert_eq!(plan.state_after(0).uid_validity, 7);
    }

    #[test]
    fn uid_sets() {
        assert_eq!(uid_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[5]), "5");
        assert_eq!(uid_set(&[]), "");
        assert_eq!(
            uid_set(&[u32::MAX - 1, u32::MAX]),
            format!("{}:{}", u32::MAX - 1, u32::MAX)
        );
        assert_eq!(sorted_uids(&[3, 1, 3]).unwrap(), [1, 3]);
        assert!(sorted_uids(&[]).is_err());
        assert!(sorted_uids(&[0, 2]).is_err());
        assert_eq!(
            expand_uid_set(&[UidSetMember::Uid(4), UidSetMember::UidRange(7..=9)]),
            [4, 7, 8, 9]
        );
        #[expect(
            clippy::reversed_empty_ranges,
            reason = "servers may send ranges high:low"
        )]
        let reversed = UidSetMember::UidRange(9..=7);
        assert_eq!(expand_uid_set(&[reversed]), [9, 8, 7]);
    }

    #[test]
    fn uid_ranges() {
        assert_eq!(UidRange::ALL.to_string(), "1:*");
        assert_eq!(UidRange::from(0).to_string(), "1:*");
        assert_eq!(UidRange::new(9, 3).to_string(), "3:9");
        assert!(UidRange::from(5).contains(5));
        assert!(!UidRange::from(5).contains(4));
        assert!(!UidRange::new(1, 3).contains(4));
    }

    #[test]
    fn internal_dates() {
        assert_eq!(
            internal_date_to_rfc3339("28-Sep-2026 08:15:00 +0200").as_deref(),
            Some("2026-09-28T08:15:00+02:00")
        );
        assert_eq!(
            internal_date_to_rfc3339(" 1-Jan-2026 00:00:00 +0000").as_deref(),
            Some("2026-01-01T00:00:00Z")
        );
        assert_eq!(
            internal_date_to_rfc3339("22-Sep-2026 17:42:10 -0400").as_deref(),
            Some("2026-09-22T17:42:10-04:00")
        );
        for bad in [
            "",
            "garbage",
            "32-Sep-2026 00:00:00 +0000",
            "01-Foo-2026 00:00:00 +0000",
            "01-Sep-2026 00:00 +0000",
            "01-Sep-2026 00:00:00 0000",
        ] {
            assert_eq!(internal_date_to_rfc3339(bad), None, "{bad}");
        }
    }

    #[test]
    fn quoting() {
        assert_eq!(quote("INBOX").unwrap(), "\"INBOX\"");
        assert_eq!(quote(r#"a"b\c"#).unwrap(), r#""a\"b\\c""#);
        assert!(quote("a\r\nb").is_err());
        assert!(matches!(astring("sam").unwrap(), Part::Text(_)));
        assert!(matches!(astring("pässword").unwrap(), Part::Literal(_)));
        assert!(astring("a\nb").is_err());
    }

    #[test]
    fn response_texts_keep_codes() {
        assert_eq!(
            response_text(Some(&ResponseCode::TryCreate), Some("no mailbox")),
            "[TRYCREATE] no mailbox"
        );
        assert_eq!(response_text(None, Some("[TOOBIG] big")), "[TOOBIG] big");
        assert_eq!(response_text(None, None), "");
    }

    #[test]
    fn fetch_conversion() {
        let attributes = vec![
            AttributeValue::Uid(7),
            AttributeValue::Flags(vec![
                Cow::Borrowed("\\Seen"),
                Cow::Borrowed("\\Recent"),
                Cow::Borrowed("\\Deleted"),
                Cow::Borrowed("$Forwarded"),
            ]),
            AttributeValue::Rfc822Size(3),
            AttributeValue::ModSeq(42),
            AttributeValue::BodySection {
                section: None,
                index: None,
                data: Some(Cow::Borrowed(b"abc")),
            },
        ];
        let message = convert_fetch(&attributes).unwrap();
        assert_eq!(message.uid, 7);
        assert!(message.flags.seen && !message.flags.flagged && message.deleted);
        assert_eq!(message.keywords, ["$Forwarded"]);
        assert_eq!(message.modseq, Some(42));
        assert_eq!(message.raw, b"abc");
        assert!(convert_fetch(&[AttributeValue::Uid(1)]).is_none());
        assert!(convert_fetch(&[AttributeValue::Rfc822Size(1)]).is_none());
    }
}
