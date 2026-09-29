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
    /// Identifies an account (real or, later, virtual).
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
    }
}
