//! The engine seams of Exchange accounts (M3c, ADR 0072; Rust only).
//!
//! Gmail fits [`crate::engine::MailApiEngine`] (one cursor per account).
//! Exchange keeps a cursor **per folder** (Graph `messages/delta` per
//! folder, EWS `SyncFolderItems`), a separate folder-hierarchy cursor,
//! several mailboxes per account (shared / delegate), and carries
//! calendars, contacts, tasks, categories, rules and automatic replies on
//! the same connection. Both engines — `nail-proto-graph` (WP91) and
//! `nail-proto-ews` (WP92) — implement [`ExchangeEngine`], the sum of:
//!
//! | Trait | What |
//! | --- | --- |
//! | [`ExchangeMailEngine`] | folder hierarchy and items per folder (deltas), MIME, replayed writes, Focused Inbox, push |
//! | [`ExchangeSettingsEngine`] | master categories, automatic replies, inbox rules |
//! | [`ExchangeDirectoryEngine`] | room lists and rooms, access checks for other mailboxes |
//! | [`ExchangePimEngine`] | calendars / contact folders / task lists with deltas, item writes, meeting responses |
//! | [`crate::engine::FreeBusyProvider`] | free/busy of people and rooms (Graph `getSchedule`, EWS `GetUserAvailability`) |
//! | [`crate::engine::OutgoingTransport`] | sending raw MIME (Graph `sendMail` with MIME, EWS `CreateItem` `SendAndSaveCopy`) |
//!
//! `mailbox` arguments name a shared / delegate mailbox (its address);
//! `None` is the account's own. Cursors are opaque strings the core stores
//! per folder / collection (`exchange_folders.cursor`,
//! `exchange_pim_collections.cursor`); a cursor the server no longer
//! accepts is [`DeltaOutcome::CursorExpired`] (full resync of that folder).
//! Throttling is [`crate::ErrorCode::RateLimited`] with
//! [`crate::engine::EngineError::retry_after_secs`].
//!
//! Windows integrated sign-in for EWS goes through [`NegotiateAuth`]
//! (implemented with SSPI by `nail-exchange`, WP93), so the EWS engine
//! needs no platform code of its own.

use std::fmt;

use super::pim::{ExchangePimFolder, ExchangePimItem};
use super::rules::ExchangeInboxRule;
use super::{
    AutomaticRepliesSettings, CategoryColor, ExchangeCategorySettings,
    ExchangeEngine as EngineKind, ExchangeWellKnownFolder, InferenceClassification, Room, RoomList,
};
use crate::engine::{
    BoxFuture, DeltaOutcome, EngineError, FreeBusyProvider, OutgoingTransport, RemoteWrite,
    WriteOutcome,
};
use crate::scheduling::InvitationResponse;

/// What the server says about itself and the signed-in mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ExchangeServerInfo {
    /// The mailbox's primary address.
    pub address: String,
    /// The user's display name.
    pub display_name: Option<String>,
    /// The server version (EWS `ServerVersionInfo`), if reported.
    pub server_version: Option<String>,
    /// The mailbox's time zone (Windows or IANA name), if known.
    pub time_zone: Option<String>,
    /// Whether the server classifies mail for Focused Inbox.
    pub focused_inbox: bool,
}

/// What a folder holds (Graph folder resources, EWS `FolderClass`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ExchangeFolderClass {
    /// Mail (`IPF.Note`).
    Mail,
    /// Calendar (`IPF.Appointment`).
    Calendar,
    /// Contacts (`IPF.Contact`).
    Contacts,
    /// Tasks (`IPF.Task`).
    Tasks,
    /// Anything else (notes, journal, search folders); not synced.
    Other,
}

/// A folder of a mailbox.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExchangeFolder {
    /// The server's id.
    pub remote_id: String,
    /// The version.
    pub change_key: Option<String>,
    /// The parent's id (`None` = below the mailbox root).
    pub parent: Option<String>,
    /// Display name.
    pub name: String,
    /// A well-known folder.
    pub well_known: Option<ExchangeWellKnownFolder>,
    /// What it holds.
    pub class: ExchangeFolderClass,
    /// Items, if the server says.
    pub total: Option<u32>,
    /// Unread items, if the server says.
    pub unread: Option<u32>,
}

/// Folder-hierarchy changes since a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct HierarchyDelta {
    /// The cursor after these changes (store it after applying them).
    pub cursor: String,
    /// New or changed folders.
    pub upserted: Vec<ExchangeFolder>,
    /// Ids of folders that are gone.
    pub removed: Vec<String>,
    /// More changes wait: call again with `cursor`.
    pub more: bool,
}

/// The answer to [`ExchangeMailEngine::folder_hierarchy`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HierarchyOutcome {
    /// The changes.
    Delta(HierarchyDelta),
    /// The cursor is no longer valid: list again from `None`.
    CursorExpired,
}

/// Mail of Exchange accounts.
pub trait ExchangeMailEngine: Send + Sync + fmt::Debug {
    /// The signed-in mailbox and server.
    fn server_info(&self) -> BoxFuture<'_, Result<ExchangeServerInfo, EngineError>>;

    /// Folder changes of a mailbox since `cursor` (`None` = every folder).
    fn folder_hierarchy<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<HierarchyOutcome, EngineError>>;

    /// Message changes in one folder since `cursor` (`None` = start a full
    /// sync of the folder; `since` limits it to messages newer than this
    /// Unix time, for "recent N days" policies). Pages arrive as deltas
    /// with `more`; the first complete pass yields the cursor for
    /// incremental sync. [`crate::engine::RemoteMessage::folder`] is the
    /// folder's remote id; Exchange data goes to
    /// [`crate::engine::RemoteMessage::exchange`].
    fn folder_items<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a str,
        cursor: Option<&'a str>,
        since: Option<i64>,
    ) -> BoxFuture<'a, Result<DeltaOutcome, EngineError>>;

    /// The raw RFC 5322 bytes of a message.
    fn fetch_mime<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_id: &'a str,
    ) -> BoxFuture<'a, Result<Vec<u8>, EngineError>>;

    /// Replays one queued write (idempotent; tags are categories). A move
    /// answers the message's new id ([`WriteOutcome::Done`]) — Exchange
    /// item ids change when an item moves.
    fn write<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a RemoteWrite,
    ) -> BoxFuture<'a, Result<WriteOutcome, EngineError>>;

    /// Moves messages between Focused and Other; with `always`, also adds
    /// an override for their senders (future mail from them lands there
    /// too).
    fn classify<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        remote_ids: &'a [String],
        classification: InferenceClassification,
        always: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>>;

    /// Waits up to `timeout_secs` for changes in these folders (EWS
    /// streaming notifications). Answers the remote ids of folders that
    /// changed (empty = none before the timeout), or `None` when the engine
    /// has no push (Graph): the core then polls with
    /// [`crate::engine::AdaptivePoll`].
    fn wait_for_changes<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folders: &'a [String],
        timeout_secs: u32,
    ) -> BoxFuture<'a, Result<Option<Vec<String>>, EngineError>>;
}

/// A category of the master list as the server has it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteCategory {
    /// The server's id (Graph id; EWS: the category's GUID in the
    /// `CategoryList` configuration).
    pub id: String,
    /// Name.
    pub name: String,
    /// Colour.
    pub color: CategoryColor,
}

/// Categories, automatic replies and inbox rules.
pub trait ExchangeSettingsEngine: Send + Sync + fmt::Debug {
    /// The master category list.
    fn categories(&self) -> BoxFuture<'_, Result<Vec<RemoteCategory>, EngineError>>;

    /// Creates a category; answers its id.
    fn create_category<'a>(
        &'a self,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<String, EngineError>>;

    /// Changes a category (renaming does not retag messages on the server;
    /// the core retags them through the mail queue). Answers the category's
    /// new id when the change gave it one (Graph cannot rename a category:
    /// a rename creates the new one and deletes the old), else `None`.
    fn update_category<'a>(
        &'a self,
        id: &'a str,
        settings: &'a ExchangeCategorySettings,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>>;

    /// Deletes a category (an unknown id is not an error).
    fn delete_category<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>>;

    /// The automatic replies.
    fn automatic_replies(&self) -> BoxFuture<'_, Result<AutomaticRepliesSettings, EngineError>>;

    /// Replaces the automatic replies.
    fn set_automatic_replies<'a>(
        &'a self,
        settings: &'a AutomaticRepliesSettings,
    ) -> BoxFuture<'a, Result<(), EngineError>>;

    /// Every inbox rule, by sequence (`nail_rule_id` is filled by the core
    /// from its own records).
    fn inbox_rules(&self) -> BoxFuture<'_, Result<Vec<ExchangeInboxRule>, EngineError>>;

    /// Creates an inbox rule; answers its id.
    fn create_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<String, EngineError>>;

    /// Replaces an inbox rule (by [`ExchangeInboxRule::id`]). Rules with
    /// [`ExchangeInboxRule::has_unsupported`] only change `enabled`.
    fn update_inbox_rule<'a>(
        &'a self,
        rule: &'a ExchangeInboxRule,
    ) -> BoxFuture<'a, Result<(), EngineError>>;

    /// Deletes an inbox rule (an unknown id is not an error).
    fn delete_inbox_rule<'a>(&'a self, id: &'a str) -> BoxFuture<'a, Result<(), EngineError>>;
}

/// Directory lookups.
pub trait ExchangeDirectoryEngine: Send + Sync + fmt::Debug {
    /// The organization's room lists (`forbidden` without the
    /// permission).
    fn room_lists(&self) -> BoxFuture<'_, Result<Vec<RoomList>, EngineError>>;

    /// The rooms of a room list, or every room (`None`).
    fn rooms<'a>(
        &'a self,
        room_list: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<Room>, EngineError>>;

    /// Checks that the user can open another mailbox and answers its
    /// display name (`forbidden` = no access, `not_found` = no mailbox).
    fn check_mailbox<'a>(
        &'a self,
        address: &'a str,
    ) -> BoxFuture<'a, Result<Option<String>, EngineError>>;
}

/// Item changes of a PIM folder since a cursor.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct PimDelta {
    /// The cursor after these changes.
    pub cursor: String,
    /// New or changed items (a series with its exceptions).
    pub upserted: Vec<ExchangePimItem>,
    /// Ids of items that are gone.
    pub removed: Vec<String>,
    /// More changes wait.
    pub more: bool,
}

/// The answer to [`ExchangePimEngine::pim_changes`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PimDeltaOutcome {
    /// The changes.
    Delta(PimDelta),
    /// Start again from `None`.
    CursorExpired,
}

/// Whom a meeting change is sent to (EWS
/// `SendMeetingInvitationsOrCancellations`; Graph sends to everyone).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum MeetingSend {
    /// Nobody (not a meeting, or "save without sending").
    #[default]
    None,
    /// Every attendee.
    ToAll,
    /// Only added / removed attendees (EWS; Graph: everyone).
    ToChanged,
}

/// A PIM write as an engine replays it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangePimWrite {
    /// Create an item in a folder.
    Create {
        /// The folder.
        folder: String,
        /// The item (`remote_id` / `change_key` ignored).
        item: ExchangePimItem,
        /// Invitations for a new meeting.
        send: MeetingSend,
    },
    /// Replace an item (by its `remote_id`; a stale `change_key` is a
    /// conflict).
    Update {
        /// The item.
        item: ExchangePimItem,
        /// Updates for a meeting.
        send: MeetingSend,
    },
    /// Delete an item (a meeting the user organizes is cancelled for the
    /// attendees per `send`).
    Delete {
        /// The item.
        remote_id: String,
        /// Cancellations.
        send: MeetingSend,
    },
}

/// What a replayed [`ExchangePimWrite`] did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExchangePimWriteOutcome {
    /// Done; the item's id and version now.
    Done {
        /// The id (new for a create).
        remote_id: String,
        /// The version.
        change_key: Option<String>,
    },
    /// The item changed on the server since (stale change key): the server
    /// wins, the local version is kept as a conflict copy.
    Conflict,
    /// The item is gone.
    Gone,
}

/// Calendars, contacts and tasks.
pub trait ExchangePimEngine: Send + Sync + fmt::Debug {
    /// The calendars, contact folders and task lists of a mailbox (Graph:
    /// To Do lists for the own mailbox's tasks).
    fn pim_folders<'a>(
        &'a self,
        mailbox: Option<&'a str>,
    ) -> BoxFuture<'a, Result<Vec<ExchangePimFolder>, EngineError>>;

    /// Item changes of a folder since `cursor` (`None` = everything).
    fn pim_changes<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        folder: &'a ExchangePimFolder,
        cursor: Option<&'a str>,
    ) -> BoxFuture<'a, Result<PimDeltaOutcome, EngineError>>;

    /// Replays one queued PIM write.
    fn write_pim<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        write: &'a ExchangePimWrite,
    ) -> BoxFuture<'a, Result<ExchangePimWriteOutcome, EngineError>>;

    /// Answers a meeting invitation in the user's calendar (Graph
    /// `accept` / `tentativelyAccept` / `decline`, EWS
    /// `AcceptItem` / …), optionally without sending the response.
    fn respond_to_meeting<'a>(
        &'a self,
        mailbox: Option<&'a str>,
        event_remote_id: &'a str,
        response: InvitationResponse,
        comment: Option<&'a str>,
        send: bool,
    ) -> BoxFuture<'a, Result<(), EngineError>>;
}

/// An Exchange engine: everything an account needs (see the module docs).
/// The core holds it as `Arc<dyn ExchangeEngine>`.
pub trait ExchangeEngine:
    ExchangeMailEngine
    + ExchangeSettingsEngine
    + ExchangeDirectoryEngine
    + ExchangePimEngine
    + FreeBusyProvider
    + OutgoingTransport
{
    /// Graph or EWS.
    fn kind(&self) -> EngineKind;
}

// ---------------------------------------------------------------------------
// Windows integrated sign-in

/// One step of a Negotiate (SPNEGO: Kerberos or NTLM) handshake.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NegotiateStep {
    /// Send this token (`Authorization: Negotiate <base64>`) and feed the
    /// server's answer back.
    Continue(Vec<u8>),
    /// Authenticated; send this last token if there is one.
    Done(Option<Vec<u8>>),
}

/// A security context in progress (one per HTTP connection: NTLM binds
/// the handshake to its connection).
pub trait NegotiateSession: Send + fmt::Debug {
    /// The next token, given the server's last one (`None` at the start).
    /// A rejected login is [`crate::ErrorCode::AuthFailed`].
    fn step(&mut self, server_token: Option<&[u8]>) -> Result<NegotiateStep, EngineError>;
}

/// HTTP Negotiate / NTLM / Kerberos authentication with the signed-in
/// Windows user or explicit credentials (SSPI; `nail-exchange`).
pub trait NegotiateAuth: Send + Sync + fmt::Debug {
    /// Starts a context for the service principal `spn` (`HTTP/<host>`).
    fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError>;
}

/// The service principal name of an HTTP host (`HTTP/mail.contoso.com`).
pub fn http_spn(host: &str) -> String {
    format!("HTTP/{}", host.trim_end_matches('.'))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ErrorCode;

    #[derive(Debug, Default)]
    struct Scripted {
        round: u8,
    }

    impl NegotiateSession for Scripted {
        fn step(&mut self, server_token: Option<&[u8]>) -> Result<NegotiateStep, EngineError> {
            self.round += 1;
            match (self.round, server_token) {
                (1, None) => Ok(NegotiateStep::Continue(b"negotiate".to_vec())),
                (2, Some(b"challenge")) => Ok(NegotiateStep::Done(Some(b"authenticate".to_vec()))),
                _ => Err(EngineError::permanent(ErrorCode::AuthFailed, "rejected")),
            }
        }
    }

    #[derive(Debug)]
    struct Fake;

    impl NegotiateAuth for Fake {
        fn start(&self, spn: &str) -> Result<Box<dyn NegotiateSession>, EngineError> {
            assert_eq!(spn, "HTTP/mail.contoso.example");
            Ok(Box::new(Scripted::default()))
        }
    }

    #[test]
    fn negotiate_seam_is_object_safe() {
        let auth: std::sync::Arc<dyn NegotiateAuth> = std::sync::Arc::new(Fake);
        let mut session = auth.start(&http_spn("mail.contoso.example.")).unwrap();
        assert_eq!(
            session.step(None).unwrap(),
            NegotiateStep::Continue(b"negotiate".to_vec())
        );
        assert_eq!(
            session.step(Some(b"challenge")).unwrap(),
            NegotiateStep::Done(Some(b"authenticate".to_vec()))
        );
        assert_eq!(session.step(None).unwrap_err().code, ErrorCode::AuthFailed);
    }

    #[test]
    fn meeting_send_defaults_to_none() {
        assert_eq!(MeetingSend::default(), MeetingSend::None);
        let delta = PimDelta::default();
        assert!(!delta.more && delta.upserted.is_empty());
    }
}
