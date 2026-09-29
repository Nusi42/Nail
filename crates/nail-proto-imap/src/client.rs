//! The read-only IMAP client.

use std::fmt;

use std::collections::BTreeSet;

use async_imap::imap_proto::types::{
    Address as ImapAddress, AttributeValue, Capability, Envelope as ImapEnvelope, MailboxDatum,
    ResponseCode,
};
use async_imap::imap_proto::{Response, Status};
use async_imap::{Authenticator, Session};
use nail_api::{Address, Flags};

use crate::capabilities::Capabilities;
use crate::config::{AuthMechanism, Credentials, ImapConfig};
use crate::error::{ImapError, Result};
use crate::folder::RemoteFolder;
use crate::transport::{ImapStream, establish, with_timeout};

/// Data items fetched per message.
const FETCH_QUERY: &str = "(UID FLAGS INTERNALDATE RFC822.SIZE ENVELOPE BODY.PEEK[])";
/// Maximum number of messages per `UID FETCH` command.
const FETCH_BATCH: usize = 50;

/// State of a mailbox after `EXAMINE`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct FolderStatus {
    /// UIDVALIDITY: UIDs are only comparable while this stays the same.
    pub uid_validity: u32,
    /// The UID the next arriving message will get (at least).
    pub uid_next: u32,
    /// Number of messages.
    pub exists: u32,
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

/// The subset of the IMAP `ENVELOPE` the client exposes. Strings are the raw
/// header values (RFC 2047 encoded words are *not* decoded; use `nail-mime`
/// on the raw message for display).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Envelope {
    /// Raw `Date` header.
    pub date: Option<String>,
    /// Raw `Subject` header.
    pub subject: Option<String>,
    /// `From` addresses.
    pub from: Vec<Address>,
    /// `To` addresses.
    pub to: Vec<Address>,
    /// `Cc` addresses.
    pub cc: Vec<Address>,
    /// Raw `In-Reply-To` header.
    pub in_reply_to: Option<String>,
    /// Raw `Message-ID` header (with angle brackets).
    pub message_id: Option<String>,
}

/// One fetched message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FetchedMessage {
    /// The message UID.
    pub uid: u32,
    /// System flags.
    pub flags: Flags,
    /// Keywords (non-system flags such as `$Forwarded`).
    pub keywords: Vec<String>,
    /// `INTERNALDATE` as RFC 3339, when the server sent a parsable one.
    pub internal_date: Option<String>,
    /// `RFC822.SIZE`.
    pub size: Option<u32>,
    /// Parsed `ENVELOPE`, when the server sent one.
    pub envelope: Option<Envelope>,
    /// The complete raw message (`BODY[]`).
    pub raw: Vec<u8>,
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

/// A logged-in, read-only IMAP connection.
pub struct ImapClient {
    session: Session<ImapStream>,
    capabilities: Capabilities,
    config: ImapConfig,
    /// Mailbox currently opened with `EXAMINE`.
    examined: Option<(String, FolderStatus)>,
}

impl fmt::Debug for ImapClient {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ImapClient")
            .field("host", &self.config.host)
            .field("port", &self.config.port)
            .field("capabilities", &self.capabilities)
            .field("examined", &self.examined)
            .finish_non_exhaustive()
    }
}

struct PlainAuth<'a>(&'a Credentials);

impl Authenticator for PlainAuth<'_> {
    type Response = Vec<u8>;

    fn process(&mut self, _challenge: &[u8]) -> Vec<u8> {
        // RFC 4616: [authzid] NUL authcid NUL passwd
        let mut response = Vec::with_capacity(self.0.username.len() + self.0.password.len() + 2);
        response.push(0);
        response.extend_from_slice(self.0.username.as_bytes());
        response.push(0);
        response.extend_from_slice(self.0.password.as_bytes());
        response
    }
}

impl ImapClient {
    /// Connects, secures the connection as configured and logs in.
    pub async fn connect(config: &ImapConfig, credentials: &Credentials) -> Result<Self> {
        let (stream, preauth_caps) = establish(config).await?;
        let mechanism = choose_mechanism(config.auth, &preauth_caps)?;
        let client = async_imap::Client::new(stream);
        let login = async {
            let result = match mechanism {
                AuthMechanism::Plain => client.authenticate("PLAIN", PlainAuth(credentials)).await,
                AuthMechanism::Login | AuthMechanism::Auto => {
                    client
                        .login(&credentials.username, &credentials.password)
                        .await
                }
            };
            result.map_err(|(err, _client)| match err {
                async_imap::error::Error::No(text) | async_imap::error::Error::Bad(text) => {
                    ImapError::AuthFailed(text)
                }
                other => other.into(),
            })
        };
        let session = with_timeout(config.timeout, login).await?;
        let mut client = Self {
            session,
            capabilities: preauth_caps,
            config: config.clone(),
            examined: None,
        };
        // Capabilities may change after authentication (RFC 3501 6.2).
        client.refresh_capabilities().await?;
        Ok(client)
    }

    /// The capabilities advertised after login.
    pub fn capabilities(&self) -> &Capabilities {
        &self.capabilities
    }

    /// Re-reads the capabilities from the server.
    pub async fn refresh_capabilities(&mut self) -> Result<&Capabilities> {
        let mut atoms = Vec::new();
        self.run("CAPABILITY", |response| {
            if let Response::Capabilities(caps) = response {
                atoms.extend(caps.iter().map(|cap| match cap {
                    Capability::Imap4rev1 => "IMAP4REV1".to_owned(),
                    Capability::Auth(mechanism) => format!("AUTH={mechanism}"),
                    Capability::Atom(atom) => atom.to_string(),
                }));
            }
            Ok(())
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
        self.run("LIST \"\" \"*\"", |response| {
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
            }
            Ok(())
        })
        .await?;
        Ok(folders)
    }

    /// Opens a mailbox read-only (`EXAMINE`) and returns its state.
    pub async fn examine(&mut self, path: &str) -> Result<FolderStatus> {
        self.examined = None;
        let (mut exists, mut uid_validity, mut uid_next) = (None, None, None);
        self.run(&format!("EXAMINE {}", quote(path)?), |response| {
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
                _ => {}
            }
            Ok(())
        })
        .await?;
        let uid_validity = uid_validity
            .ok_or_else(|| ImapError::Protocol(format!("no UIDVALIDITY for {path}")))?;
        let exists = exists.ok_or_else(|| ImapError::Protocol(format!("no EXISTS for {path}")))?;
        let mut status = FolderStatus {
            uid_validity,
            uid_next: 1,
            exists,
        };
        self.examined = Some((path.to_owned(), status));
        status.uid_next = match uid_next {
            Some(uid_next) => uid_next,
            // Servers must send UIDNEXT, but some old ones do not.
            None => self
                .search_uids(UidRange::ALL)
                .await?
                .last()
                .map_or(1, |uid| uid + 1),
        };
        self.examined = Some((path.to_owned(), status));
        Ok(status)
    }

    /// Fetches all messages of the examined mailbox whose UID lies in
    /// `range` (raw bytes, flags, envelope), in UID order, in batches.
    /// Call [`Self::examine`] first.
    pub async fn fetch_range(&mut self, range: UidRange) -> Result<Vec<FetchedMessage>> {
        if self.examined.is_none() {
            return Err(ImapError::Protocol("no mailbox examined".to_owned()));
        }
        let uids = self.search_uids(range).await?;
        let mut messages = Vec::with_capacity(uids.len());
        for batch in uids.chunks(FETCH_BATCH) {
            let mut fetched = Vec::with_capacity(batch.len());
            self.run(
                &format!("UID FETCH {} {FETCH_QUERY}", uid_set(batch)),
                |response| {
                    if let Response::Fetch(_, attributes) = response {
                        // Unsolicited FETCH responses (flag updates) for other
                        // messages may be interleaved; keep only what was asked.
                        if let Some(message) = convert_fetch(attributes)
                            .filter(|m| batch.binary_search(&m.uid).is_ok())
                        {
                            fetched.push(message);
                        }
                    }
                    Ok(())
                },
            )
            .await?;
            self.check_complete(batch, &fetched).await?;
            messages.extend(fetched);
        }
        messages.sort_by_key(|m| m.uid);
        messages.dedup_by_key(|m| m.uid);
        Ok(messages)
    }

    /// A requested UID without a response is fine only if the message was
    /// expunged meanwhile; otherwise the server failed us silently.
    async fn check_complete(
        &mut self,
        requested: &[u32],
        fetched: &[FetchedMessage],
    ) -> Result<()> {
        let got: BTreeSet<u32> = fetched.iter().map(|m| m.uid).collect();
        let missing: Vec<u32> = requested
            .iter()
            .copied()
            .filter(|uid| !got.contains(uid))
            .collect();
        if missing.is_empty() {
            return Ok(());
        }
        let mut still_there = Vec::new();
        self.run(
            &format!("UID SEARCH UID {}", uid_set(&missing)),
            |response| {
                if let Response::MailboxData(MailboxDatum::Search(uids)) = response {
                    still_there.extend(
                        uids.iter()
                            .copied()
                            .filter(|uid| missing.binary_search(uid).is_ok()),
                    );
                }
                Ok(())
            },
        )
        .await?;
        if still_there.is_empty() {
            Ok(())
        } else {
            still_there.sort_unstable();
            Err(ImapError::Protocol(format!(
                "server did not return messages {}",
                uid_set(&still_there)
            )))
        }
    }

    /// Examines `path` and fetches what is new since `known` (a previous
    /// [`IncrementalFetch::next_state`]). Without `known`, or when the
    /// server's UIDVALIDITY changed, the whole mailbox is fetched.
    pub async fn fetch_new(
        &mut self,
        path: &str,
        known: Option<SyncState>,
    ) -> Result<IncrementalFetch> {
        let status = self.examine(path).await?;
        let uid_validity_changed = known.is_some_and(|k| k.uid_validity != status.uid_validity);
        let range = match known {
            Some(known) if !uid_validity_changed => UidRange::from(known.uid_next),
            _ => UidRange::ALL,
        };
        let nothing_new =
            matches!(known, Some(k) if !uid_validity_changed && status.uid_next <= k.uid_next);
        let messages = if nothing_new {
            Vec::new()
        } else {
            self.fetch_range(range).await?
        };

        let fetched_next = messages.last().map_or(0, |m| m.uid.saturating_add(1));
        let known_next = known
            .filter(|_| !uid_validity_changed)
            .map_or(1, |k| k.uid_next);
        Ok(IncrementalFetch {
            status,
            uid_validity_changed,
            next_state: SyncState {
                uid_validity: status.uid_validity,
                uid_next: status.uid_next.max(fetched_next).max(known_next),
            },
            messages,
        })
    }

    /// Sends `NOOP` (keep-alive / poll for changes).
    pub async fn noop(&mut self) -> Result<()> {
        self.run("NOOP", |_| Ok(())).await
    }

    /// Logs out and closes the connection.
    pub async fn logout(mut self) -> Result<()> {
        self.run("LOGOUT", |_| Ok(())).await
    }

    /// UIDs in `range` in the examined mailbox, ascending. Filters the
    /// result, because `n:*` always matches the highest UID even when it is
    /// below `n` (RFC 3501 6.4.8).
    async fn search_uids(&mut self, range: UidRange) -> Result<Vec<u32>> {
        let mut uids = Vec::new();
        self.run(&format!("UID SEARCH UID {range}"), |response| {
            if let Response::MailboxData(MailboxDatum::Search(found)) = response {
                uids.extend(found.iter().copied().filter(|uid| range.contains(*uid)));
            }
            Ok(())
        })
        .await?;
        uids.sort_unstable();
        uids.dedup();
        Ok(uids)
    }

    /// Sends a command and feeds every untagged response to `on_untagged`
    /// until the tagged completion, which must be `OK`.
    ///
    /// `async-imap`'s streaming helpers end silently on EOF and ignore the
    /// tagged status of `FETCH`/`SEARCH`/`LIST`; this runner turns a closed
    /// connection into [`ImapError::ConnectionLost`] and `NO`/`BAD` into errors.
    async fn run(
        &mut self,
        command: &str,
        mut on_untagged: impl FnMut(&Response<'_>) -> Result<()>,
    ) -> Result<()> {
        let session = &mut self.session;
        with_timeout(self.config.timeout, async {
            let tag = session.run_command(command).await?;
            loop {
                let Some(response) = session.read_response().await? else {
                    return Err(ImapError::ConnectionLost);
                };
                match response.parsed() {
                    Response::Done {
                        tag: done,
                        status,
                        information,
                        ..
                    } if *done == tag => {
                        let text = information.as_deref().unwrap_or_default().to_owned();
                        return match status {
                            Status::Ok => Ok(()),
                            Status::No => Err(ImapError::No(text)),
                            other => Err(ImapError::Protocol(format!("{other:?} {text}"))),
                        };
                    }
                    other => on_untagged(other)?,
                }
            }
        })
        .await
    }
}

/// Quotes a mailbox name as an IMAP quoted string.
fn quote(value: &str) -> Result<String> {
    if value.contains(['\r', '\n', '\0']) {
        return Err(ImapError::Protocol(
            "mailbox name contains control characters".to_owned(),
        ));
    }
    Ok(format!(
        "\"{}\"",
        value.replace('\\', "\\\\").replace('"', "\\\"")
    ))
}

fn choose_mechanism(requested: AuthMechanism, caps: &Capabilities) -> Result<AuthMechanism> {
    let login_disabled = caps.has("LOGINDISABLED");
    match requested {
        AuthMechanism::Plain => Ok(AuthMechanism::Plain),
        AuthMechanism::Login if login_disabled => {
            Err(ImapError::AuthUnsupported("LOGINDISABLED".to_owned()))
        }
        AuthMechanism::Login => Ok(AuthMechanism::Login),
        AuthMechanism::Auto if caps.has_auth("PLAIN") => Ok(AuthMechanism::Plain),
        AuthMechanism::Auto if login_disabled => Err(ImapError::AuthUnsupported(
            "LOGINDISABLED without AUTH=PLAIN".to_owned(),
        )),
        AuthMechanism::Auto => Ok(AuthMechanism::Login),
    }
}

/// Compresses sorted UIDs into an IMAP set: `1:3,7,9:10`.
fn uid_set(uids: &[u32]) -> String {
    let mut out = String::new();
    let mut iter = uids.iter().copied().peekable();
    while let Some(start) = iter.next() {
        let mut end = start;
        while iter.peek() == Some(&end.wrapping_add(1)) {
            end = iter.next().unwrap_or(end);
        }
        if !out.is_empty() {
            out.push(',');
        }
        if start == end {
            out.push_str(&start.to_string());
        } else {
            out.push_str(&format!("{start}:{end}"));
        }
    }
    out
}

/// Converts the attributes of one `FETCH` response; `None` without UID or body.
fn convert_fetch(attributes: &[AttributeValue<'_>]) -> Option<FetchedMessage> {
    let mut uid = None;
    let mut flags = Flags::default();
    let mut keywords = Vec::new();
    let mut internal_date = None;
    let mut size = None;
    let mut envelope = None;
    let mut raw = None;
    for attribute in attributes {
        match attribute {
            AttributeValue::Uid(value) => uid = Some(*value),
            AttributeValue::Flags(values) => {
                for flag in values {
                    match flag.to_ascii_lowercase().as_str() {
                        "\\seen" => flags.seen = true,
                        "\\flagged" => flags.flagged = true,
                        "\\answered" => flags.answered = true,
                        "\\draft" => flags.draft = true,
                        other if other.starts_with('\\') => {}
                        _ => keywords.push(flag.to_string()),
                    }
                }
            }
            AttributeValue::InternalDate(value) => internal_date = internal_date_to_rfc3339(value),
            AttributeValue::Rfc822Size(value) => size = Some(*value),
            AttributeValue::Envelope(value) => envelope = Some(convert_envelope(value)),
            AttributeValue::BodySection {
                section: None,
                data: Some(data),
                ..
            }
            | AttributeValue::Rfc822(Some(data)) => raw = Some(data.to_vec()),
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
        flags,
        keywords,
        internal_date,
        size,
        envelope,
        raw,
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

fn lossy(bytes: Option<&std::borrow::Cow<'_, [u8]>>) -> Option<String> {
    bytes.map(|b| String::from_utf8_lossy(b).into_owned())
}

fn convert_envelope(envelope: &ImapEnvelope<'_>) -> Envelope {
    Envelope {
        date: lossy(envelope.date.as_ref()),
        subject: lossy(envelope.subject.as_ref()),
        from: convert_addresses(envelope.from.as_deref()),
        to: convert_addresses(envelope.to.as_deref()),
        cc: convert_addresses(envelope.cc.as_deref()),
        in_reply_to: lossy(envelope.in_reply_to.as_ref()),
        message_id: lossy(envelope.message_id.as_ref()),
    }
}

fn convert_addresses(addresses: Option<&[ImapAddress<'_>]>) -> Vec<Address> {
    addresses
        .unwrap_or_default()
        .iter()
        .filter_map(|addr| {
            // Group syntax markers have no host.
            let mailbox = lossy(addr.mailbox.as_ref())?;
            let host = lossy(addr.host.as_ref())?;
            Some(Address {
                name: lossy(addr.name.as_ref()).filter(|n| !n.is_empty()),
                email: format!("{mailbox}@{host}"),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn uid_sets() {
        assert_eq!(uid_set(&[1, 2, 3, 7, 9, 10]), "1:3,7,9:10");
        assert_eq!(uid_set(&[5]), "5");
        assert_eq!(uid_set(&[]), "");
        assert_eq!(
            uid_set(&[u32::MAX - 1, u32::MAX]),
            format!("{}:{}", u32::MAX - 1, u32::MAX)
        );
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
    fn mechanism_choice() {
        let plain = Capabilities::from_atoms(["IMAP4rev1", "AUTH=PLAIN"]);
        let none = Capabilities::from_atoms(["IMAP4rev1"]);
        let disabled = Capabilities::from_atoms(["IMAP4rev1", "LOGINDISABLED"]);
        assert_eq!(
            choose_mechanism(AuthMechanism::Auto, &plain).unwrap(),
            AuthMechanism::Plain
        );
        assert_eq!(
            choose_mechanism(AuthMechanism::Auto, &none).unwrap(),
            AuthMechanism::Login
        );
        assert!(choose_mechanism(AuthMechanism::Auto, &disabled).is_err());
        assert!(choose_mechanism(AuthMechanism::Login, &disabled).is_err());
        assert_eq!(
            choose_mechanism(AuthMechanism::Plain, &none).unwrap(),
            AuthMechanism::Plain
        );
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
    }

    #[test]
    fn fetch_conversion() {
        use std::borrow::Cow;
        let attributes = vec![
            AttributeValue::Uid(7),
            AttributeValue::Flags(vec![
                Cow::Borrowed("\\Seen"),
                Cow::Borrowed("\\Recent"),
                Cow::Borrowed("$Forwarded"),
            ]),
            AttributeValue::Rfc822Size(3),
            AttributeValue::BodySection {
                section: None,
                index: None,
                data: Some(Cow::Borrowed(b"abc")),
            },
        ];
        let message = convert_fetch(&attributes).unwrap();
        assert_eq!(message.uid, 7);
        assert!(message.flags.seen && !message.flags.flagged);
        assert_eq!(message.keywords, ["$Forwarded"]);
        assert_eq!(message.raw, b"abc");
        assert!(convert_fetch(&[AttributeValue::Uid(1)]).is_none());
        assert!(convert_fetch(&[AttributeValue::Rfc822Size(1)]).is_none());
    }

    #[test]
    fn plain_response_format() {
        let creds = Credentials::new("sam", "pw");
        assert_eq!(PlainAuth(&creds).process(b""), b"\0sam\0pw");
    }
}
