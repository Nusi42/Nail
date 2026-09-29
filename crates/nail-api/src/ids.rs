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
        assert_eq!(
            serde_json::to_value(OpId::new("op-7")).unwrap(),
            serde_json::json!("op-7")
        );
    }
}
