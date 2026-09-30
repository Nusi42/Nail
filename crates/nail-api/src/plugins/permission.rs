//! Plug-in permissions and grants (design.md, "Permissions and sandbox";
//! the preview's subset, plan M2d WP60).
//!
//! A manifest declares the permissions it needs; the install dialog shows
//! them and the user grants them (all, or mail access limited to some
//! accounts). The host's permission gate checks every API call against
//! the grants. In the preview **every** capability needs its permission,
//! including dialogs, notifications and storage (the design lets those go
//! without one; the preview lists them so the install dialog names every
//! capability — ADR 0050).
//!
//! On the wire a permission is its name: `"mail.read"`,
//! `"network:api.example.com"`, `"network:*.example.org"`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::AccountId;
use crate::localized::LocalizedMessage;

/// A permission of the preview API.
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, TS)]
#[ts(
    type = "\"mail.read\" | \"mail.read.body\" | \"calendar.read\" | \"ui.dialogs\" | \"notifications\" | \"storage\" | `network:${string}`"
)]
pub enum PluginPermission {
    /// `mail.read`: accounts, folders and message headers (`nail.mail`),
    /// never bodies. Can be limited to accounts.
    MailRead,
    /// `mail.read.body`: message bodies too (needs `mail.read`). Never
    /// the decrypted content of end-to-end encrypted mail (that is
    /// `mail.read.encrypted`, M4). Can be limited to accounts.
    MailReadBody,
    /// `calendar.read`: a placeholder until the PIM API (M3); accepted in
    /// manifests and grants, grants nothing yet.
    CalendarRead,
    /// `ui.dialogs`: open dialogs (`nail.ui.showDialog`).
    UiDialogs,
    /// `notifications`: in-app and OS notifications (`nail.ui.notify`).
    Notifications,
    /// `storage`: the plug-in's own key-value storage (`nail.storage`),
    /// within its quota.
    Storage,
    /// `network:<domain>`: `nail.http.fetch` to one host pattern, through
    /// the core. `api.example.com` is that host only; `*.example.com` is
    /// every subdomain (not `example.com` itself). No scheme, port or path.
    Network(String),
}

/// Why a permission name is not accepted.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum PluginPermissionError {
    /// Not a permission name at all.
    #[error("unknown permission `{0}`")]
    Unknown(String),
    /// A permission of the full API (M4), not available in the preview.
    #[error("permission `{0}` is not available in the plug-in preview")]
    NotInPreview(String),
    /// `network:` with an invalid host pattern.
    #[error("invalid network domain `{0}` (expected a host name or `*.` + a domain)")]
    InvalidDomain(String),
}

/// Permission names of the full API (design.md) that the preview refuses
/// with [`PluginPermissionError::NotInPreview`].
pub const FUTURE_PERMISSIONS: &[&str] = &[
    "mail.read.encrypted",
    "mail.organize",
    "mail.send",
    "calendar.write",
    "contacts.read",
    "contacts.write",
    "tasks.read",
    "tasks.write",
    "hooks.arrival",
    "hooks.beforeSend",
    "ui.webview",
    "mcp.expose",
];

/// Prefix of network permissions.
pub const NETWORK_PREFIX: &str = "network:";

impl PluginPermission {
    /// The permissions without an argument, in display order.
    pub const FIXED: [PluginPermission; 6] = [
        Self::MailRead,
        Self::MailReadBody,
        Self::CalendarRead,
        Self::UiDialogs,
        Self::Notifications,
        Self::Storage,
    ];

    /// Parses a permission name.
    pub fn parse(name: &str) -> Result<Self, PluginPermissionError> {
        Ok(match name {
            "mail.read" => Self::MailRead,
            "mail.read.body" => Self::MailReadBody,
            "calendar.read" => Self::CalendarRead,
            "ui.dialogs" => Self::UiDialogs,
            "notifications" => Self::Notifications,
            "storage" => Self::Storage,
            _ => {
                if let Some(domain) = name.strip_prefix(NETWORK_PREFIX) {
                    if !is_domain_pattern(domain) {
                        return Err(PluginPermissionError::InvalidDomain(domain.to_owned()));
                    }
                    Self::Network(domain.to_owned())
                } else if FUTURE_PERMISSIONS.contains(&name) {
                    return Err(PluginPermissionError::NotInPreview(name.to_owned()));
                } else {
                    return Err(PluginPermissionError::Unknown(name.to_owned()));
                }
            }
        })
    }

    /// The permission's name.
    pub fn name(&self) -> String {
        match self {
            Self::MailRead => "mail.read".to_owned(),
            Self::MailReadBody => "mail.read.body".to_owned(),
            Self::CalendarRead => "calendar.read".to_owned(),
            Self::UiDialogs => "ui.dialogs".to_owned(),
            Self::Notifications => "notifications".to_owned(),
            Self::Storage => "storage".to_owned(),
            Self::Network(domain) => format!("{NETWORK_PREFIX}{domain}"),
        }
    }

    /// Whether a grant of this permission can be limited to accounts
    /// ([`PluginGrant::accounts`]).
    pub fn takes_accounts(&self) -> bool {
        matches!(self, Self::MailRead | Self::MailReadBody)
    }

    /// Whether this permission is accepted but grants nothing yet.
    pub fn is_placeholder(&self) -> bool {
        matches!(self, Self::CalendarRead)
    }

    /// Whether a `network:` permission allows requests to `host` (a host
    /// name or IPv4 address, any case, optional trailing dot).
    pub fn allows_host(&self, host: &str) -> bool {
        let Self::Network(pattern) = self else {
            return false;
        };
        let host = host.trim_end_matches('.').to_ascii_lowercase();
        match pattern.strip_prefix("*.") {
            Some(parent) => host
                .strip_suffix(parent)
                .is_some_and(|sub| sub.len() > 1 && sub.ends_with('.')),
            None => host == *pattern,
        }
    }

    /// The text the install dialog and the plug-in manager show
    /// (`plugin-permission-<name>`; network permissions pass `$domain`).
    pub fn label(&self) -> LocalizedMessage {
        match self {
            Self::Network(domain) => {
                LocalizedMessage::new("plugin-permission-network").arg("domain", domain.as_str())
            }
            other => LocalizedMessage::new(other.fixed_message_id()),
        }
    }

    fn fixed_message_id(&self) -> &'static str {
        match self {
            Self::MailRead => "plugin-permission-mail-read",
            Self::MailReadBody => "plugin-permission-mail-read-body",
            Self::CalendarRead => "plugin-permission-calendar-read",
            Self::UiDialogs => "plugin-permission-ui-dialogs",
            Self::Notifications => "plugin-permission-notifications",
            Self::Storage => "plugin-permission-storage",
            Self::Network(_) => "plugin-permission-network",
        }
    }

    /// Every Fluent id [`Self::label`] can return.
    pub fn label_ids() -> Vec<&'static str> {
        let mut ids: Vec<&'static str> = Self::FIXED.iter().map(Self::fixed_message_id).collect();
        ids.push("plugin-permission-network");
        ids
    }
}

impl std::fmt::Display for PluginPermission {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.name())
    }
}

impl Serialize for PluginPermission {
    fn serialize<S: serde::Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(&self.name())
    }
}

impl<'de> Deserialize<'de> for PluginPermission {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let name = String::deserialize(deserializer)?;
        Self::parse(&name).map_err(serde::de::Error::custom)
    }
}

/// Whether `pattern` is a valid `network:` host pattern: a lowercase host
/// name (labels of `a-z`, `0-9`, `-`, at most 63 characters each, 253 in
/// all), optionally prefixed by `*.` (then at least two labels follow), or
/// an IPv4 address. No scheme, port, path, IPv6 or bare `*`.
pub fn is_domain_pattern(pattern: &str) -> bool {
    if pattern.parse::<std::net::Ipv4Addr>().is_ok() {
        return true;
    }
    let (wildcard, host) = match pattern.strip_prefix("*.") {
        Some(host) => (true, host),
        None => (false, pattern),
    };
    if host.is_empty() || host.len() > 253 {
        return false;
    }
    let labels: Vec<&str> = host.split('.').collect();
    let label_ok = |l: &&str| {
        !l.is_empty()
            && l.len() <= 63
            && l.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-')
            && !l.starts_with('-')
            && !l.ends_with('-')
    };
    // `*.com` would cover a whole top-level domain.
    labels.iter().all(label_ok)
        && (!wildcard || labels.len() >= 2)
        && !labels
            .last()
            .is_some_and(|l| l.bytes().all(|b| b.is_ascii_digit()))
}

/// A granted permission.
///
/// JSON: `{"permission": "mail.read", "accounts": ["work"]}`; `accounts`
/// is `null` (or absent) for every account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginGrant {
    /// The permission (one the manifest declares).
    pub permission: PluginPermission,
    /// For `mail.read` / `mail.read.body` only: the accounts (real or
    /// virtual) the plug-in may read; `null` = all. Must be `null` for
    /// every other permission.
    #[serde(default)]
    pub accounts: Option<Vec<AccountId>>,
}

impl PluginGrant {
    /// A grant for every account.
    pub fn all(permission: PluginPermission) -> Self {
        Self {
            permission,
            accounts: None,
        }
    }

    /// Whether the grant reaches `account` (always for grants without an
    /// account list).
    pub fn allows_account(&self, account: &AccountId) -> bool {
        self.accounts
            .as_ref()
            .is_none_or(|accounts| accounts.contains(account))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn fixed_permissions_round_trip() {
        for p in PluginPermission::FIXED {
            assert_eq!(PluginPermission::parse(&p.name()), Ok(p.clone()));
            let value = serde_json::to_value(&p).unwrap();
            assert_eq!(value, json!(p.name()));
            assert_eq!(
                serde_json::from_value::<PluginPermission>(value).unwrap(),
                p
            );
        }
    }

    #[test]
    fn network_permissions() {
        let p = PluginPermission::parse("network:api.example.com").unwrap();
        assert_eq!(p, PluginPermission::Network("api.example.com".into()));
        assert_eq!(p.to_string(), "network:api.example.com");
        assert!(p.allows_host("api.example.com"));
        assert!(p.allows_host("API.Example.com."));
        assert!(!p.allows_host("x.api.example.com"));
        assert!(!p.allows_host("example.com"));

        let wild = PluginPermission::parse("network:*.example.org").unwrap();
        assert!(wild.allows_host("a.example.org"));
        assert!(wild.allows_host("a.b.example.org"));
        assert!(!wild.allows_host("example.org"));
        assert!(!wild.allows_host("badexample.org"));
        assert!(!PluginPermission::Storage.allows_host("example.org"));

        assert!(PluginPermission::parse("network:127.0.0.1").is_ok());
        assert!(PluginPermission::parse("network:localhost").is_ok());
        for bad in [
            "network:",
            "network:*",
            "network:*.com",
            "network:https://example.com",
            "network:example.com:443",
            "network:example.com/path",
            "network:Example.com",
            "network:-a.com",
            "network:a..com",
            "network:a.b.*.com",
            "network:1.2.3",
            "network:[::1]",
        ] {
            assert!(
                matches!(
                    PluginPermission::parse(bad),
                    Err(PluginPermissionError::InvalidDomain(_))
                ),
                "{bad}"
            );
        }
    }

    #[test]
    fn unknown_and_future_permissions_are_refused() {
        assert_eq!(
            PluginPermission::parse("mail.send"),
            Err(PluginPermissionError::NotInPreview("mail.send".into()))
        );
        assert_eq!(
            PluginPermission::parse("everything"),
            Err(PluginPermissionError::Unknown("everything".into()))
        );
        assert!(serde_json::from_value::<PluginPermission>(json!("mail.organize")).is_err());
        for name in FUTURE_PERMISSIONS {
            assert!(PluginPermission::parse(name).is_err(), "{name}");
        }
    }

    #[test]
    fn properties_and_labels() {
        assert!(PluginPermission::MailRead.takes_accounts());
        assert!(PluginPermission::MailReadBody.takes_accounts());
        assert!(!PluginPermission::Storage.takes_accounts());
        assert!(PluginPermission::CalendarRead.is_placeholder());
        assert_eq!(
            PluginPermission::MailRead.label().id,
            "plugin-permission-mail-read"
        );
        let net = PluginPermission::Network("a.example".into()).label();
        assert_eq!(net.id, "plugin-permission-network");
        assert_eq!(net.args["domain"], "a.example".into());
        let ids = PluginPermission::label_ids();
        assert_eq!(ids.len(), 7);
        assert!(ids.iter().all(|id| crate::localized::is_message_id(id)));
    }

    #[test]
    fn grants() {
        let grant: PluginGrant =
            serde_json::from_value(json!({"permission": "mail.read"})).unwrap();
        assert_eq!(grant, PluginGrant::all(PluginPermission::MailRead));
        assert!(grant.allows_account(&"work".into()));
        let limited = PluginGrant {
            permission: PluginPermission::MailRead,
            accounts: Some(vec!["work".into()]),
        };
        assert!(limited.allows_account(&"work".into()));
        assert!(!limited.allows_account(&"home".into()));
        assert_eq!(
            serde_json::to_value(&limited).unwrap(),
            json!({"permission": "mail.read", "accounts": ["work"]})
        );
    }
}
