//! Opaque identifiers.
//!
//! All ids are strings on the wire. Their format is owned by `nail-core` and
//! must be treated as opaque by every client (UI, MCP, plug-ins): never parse
//! or construct them, only pass back what the core handed out.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

macro_rules! string_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS,
        )]
        pub struct $name(pub String);

        impl $name {
            /// Creates an id from anything convertible into a `String`.
            pub fn new(id: impl Into<String>) -> Self {
                Self(id.into())
            }

            /// Returns the id as a string slice.
            pub fn as_str(&self) -> &str {
                &self.0
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                f.write_str(&self.0)
            }
        }

        impl From<String> for $name {
            fn from(id: String) -> Self {
                Self(id)
            }
        }

        impl From<&str> for $name {
            fn from(id: &str) -> Self {
                Self(id.to_owned())
            }
        }
    };
}

string_id!(
    /// Identifies an account, real or virtual. For accounts configured in
    /// `accounts.kdl` it is the name written there (`account "work"`); the
    /// built-in "All accounts" virtual account is [`AccountId::ALL`].
    AccountId
);
string_id!(
    /// Identifies a folder (mailbox) within an account.
    FolderId
);
string_id!(
    /// Identifies a message in the local store. Stable across restarts.
    MessageId
);
string_id!(
    /// Identifies a sending identity. For identities from `accounts.kdl` it
    /// is the `id=` written there, or the identity's address when there is
    /// none. Unique across all accounts.
    IdentityId
);
string_id!(
    /// Identifies a conversation thread. Threads can span folders and, in a
    /// virtual account, accounts.
    ThreadId
);
string_id!(
    /// Identifies a draft in the local store.
    DraftId
);
string_id!(
    /// Identifies an entry of the outbox (a message waiting to be sent).
    OutboxId
);
string_id!(
    /// Identifies an operation in the core's operation queue.
    OpId
);
string_id!(
    /// Identifies a running OAuth2 authorization (see
    /// [`crate::Request::StartOAuth`]).
    OAuthFlowId
);

string_id!(
    /// Identifies a filter rule. For rules from `rules.kdl` it is the
    /// `id=` written there, or one the core derives from the name.
    RuleId
);
string_id!(
    /// Identifies one run of a rule (the unit that can be undone).
    RuleRunId
);
string_id!(
    /// Identifies an MCP client registered in Settings → MCP clients.
    McpClientId
);
string_id!(
    /// Identifies a saved search (smart folder).
    SavedSearchId
);
string_id!(
    /// Identifies a backup of a configuration file, kept when the file is
    /// rewritten by a merge or a revert.
    ConfigBackupId
);

string_id!(
    /// Identifies an OpenPGP key or S/MIME certificate in the keyring
    /// (M2a, [`crate::security::CryptoKey`]). Fingerprints are separate
    /// fields; never derive one from the other.
    KeyId
);

string_id!(
    /// Identifies a remote-UI user (M2b, [`crate::remote::RemoteUser`]).
    RemoteUserId
);
string_id!(
    /// Identifies a device paired with the remote UI
    /// ([`crate::remote::RemoteDevice`]).
    RemoteDeviceId
);
string_id!(
    /// Identifies a remote-UI session ([`crate::remote::RemoteSession`]).
    /// Not the session token: the token is secret and lives only in the
    /// browser's cookie; the core keeps a hash of it.
    RemoteSessionId
);
string_id!(
    /// Identifies a registered passkey (WebAuthn credential) of a remote
    /// user ([`crate::remote::Passkey`]).
    PasskeyId
);
string_id!(
    /// Identifies a pairing code ([`crate::remote::PairingCode`]). Not the
    /// code itself, which is shown once and stored only as a hash.
    PairingId
);
string_id!(
    /// Identifies an installed plug-in: the `id` of its manifest
    /// (`follow-up`, M2d). Unlike the other ids it is chosen by the
    /// plug-in's author, not by the core; its form is checked by
    /// [`crate::plugins::is_plugin_id`].
    PluginId
);
string_id!(
    /// Identifies an open plug-in dialog ([`crate::plugins::PluginDialog`],
    /// M2d). Opaque, made by the core.
    PluginDialogId
);

string_id!(
    /// Identifies a calendar, contact or task source (M3a,
    /// [`crate::pim::PimSource`]): the name of its node in `accounts.kdl`
    /// (`caldav "work-dav"`), or [`PimSourceId::LOCAL`] for the built-in
    /// local source. A separate name space from accounts.
    PimSourceId
);
string_id!(
    /// Identifies a calendar ([`crate::pim::Calendar`]). Task lists are
    /// calendars that hold tasks (CalDAV keeps both in one collection).
    /// Opaque, made by the core.
    CalendarId
);
string_id!(
    /// Identifies an address book ([`crate::pim::AddressBook`]). Opaque.
    AddressBookId
);
string_id!(
    /// Identifies a calendar event, the whole series of a recurring one
    /// ([`crate::pim::CalendarEvent`]). Opaque; not the iCalendar `UID`.
    EventId
);
string_id!(
    /// Identifies a contact ([`crate::pim::Contact`]). Opaque; not the
    /// vCard `UID`.
    ContactId
);
string_id!(
    /// Identifies a contact group ([`crate::pim::ContactGroup`]). Opaque.
    ContactGroupId
);
string_id!(
    /// Identifies a task ([`crate::pim::Task`]). Opaque; not the
    /// iCalendar `UID`.
    TaskId
);
string_id!(
    /// Identifies one reminder: an alarm of an event occurrence or a task
    /// ([`crate::pim::Reminder`]). Opaque.
    ReminderId
);
string_id!(
    /// Gmail's id of a label (`INBOX`, `CATEGORY_SOCIAL`, `Label_12`), or
    /// a temporary `local-…` id for a label created locally whose creation
    /// is still queued (M3b, [`crate::gmail::GmailLabel`]).
    GmailLabelId
);
string_id!(
    /// The server's id of an Exchange category (Graph `outlookCategory`
    /// id, EWS category GUID), or a temporary `local-…` id while its
    /// creation is queued (M3c, [`crate::exchange::ExchangeCategory`]).
    ExchangeCategoryId
);

string_id!(
    /// Identifies a confirmation request: a write by an MCP client or a
    /// plug-in waiting for the user's decision (M4a,
    /// [`crate::agents::Confirmation`]). Opaque, made by the core.
    ConfirmationId
);
string_id!(
    /// Identifies a before-send veto waiting for the user's "Send anyway"
    /// (M4a, [`crate::Response::SendVetoed`]). Opaque, made by the core.
    SendVetoId
);

string_id!(
    /// Identifies an import, export, backup, verification or restore job
    /// (M4c, [`crate::portability::JobStatus`]). Opaque, made by the core;
    /// jobs live in memory and are forgotten on restart.
    JobId
);
string_id!(
    /// Identifies a print job: what to print and how, read by the print
    /// view (M4c, [`crate::print::PrintJob`]). Opaque, made by the core.
    PrintJobId
);
string_id!(
    /// Identifies a smartcard PIN prompt waiting for the user (M4c,
    /// [`crate::smartcard::PinPrompt`]). Opaque, made by the core.
    PinPromptId
);

impl PimSourceId {
    /// Id of the built-in local source (local calendars, task lists and
    /// address books, including the "Collected" address book). Reserved:
    /// `accounts.kdl` cannot define a source with this name.
    pub const LOCAL: &'static str = "local";

    /// The built-in local source.
    pub fn local() -> Self {
        Self::new(Self::LOCAL)
    }
}

impl AccountId {
    /// Id of the built-in "All accounts" virtual account (the unified
    /// inbox). It is reserved: `accounts.kdl` cannot define an account or a
    /// virtual account with this name.
    pub const ALL: &'static str = "all";

    /// The built-in "All accounts" virtual account.
    pub fn all() -> Self {
        Self::new(Self::ALL)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_serialize_as_plain_strings() {
        let id = AccountId::new("acc-1");
        assert_eq!(serde_json::to_string(&id).unwrap(), "\"acc-1\"");
        let back: AccountId = serde_json::from_str("\"acc-1\"").unwrap();
        assert_eq!(back, id);
        assert_eq!(id.to_string(), "acc-1");
        assert_eq!(MessageId::from("m").as_str(), "m");
        assert_eq!(AccountId::all().as_str(), "all");
        assert_eq!(PimSourceId::local().as_str(), "local");
        assert_eq!(
            serde_json::to_value(EventId::new("ev-1")).unwrap(),
            serde_json::json!("ev-1")
        );
        assert_eq!(
            serde_json::to_value(OpId::new("op-7")).unwrap(),
            serde_json::json!("op-7")
        );
    }
}
