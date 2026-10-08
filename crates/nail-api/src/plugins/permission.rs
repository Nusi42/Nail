//! Plug-in permissions and grants (design.md, "Permissions and sandbox";
//! the preview's subset, plan M2d WP60; the writes, hooks and extension
//! points of the full API, plan M4a WP100, ADR 0079).
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
//!
//! M4a adds the writes (`mail.organize`, `mail.send`, `calendar.write`,
//! `contacts.write`, `tasks.write`; every write goes through the operation
//! queue, is audited and asks the user per the plug-in's action policies,
//! [`crate::agents`]), the mail hooks (`hooks.arrival`, `hooks.beforeSend`,
//! `hooks.open`), the extension points (`composer`, `attachments`,
//! `background`) and `mcp.expose`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::ids::AccountId;
use crate::localized::LocalizedMessage;

/// A plug-in permission (the preview's and, since M4a, the writes, hooks and
/// extension points).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, TS)]
#[ts(
    type = "\"mail.read\" | \"mail.read.body\" | \"calendar.read\" | \"ui.dialogs\" | \"notifications\" | \"storage\" | \"mail.organize\" | \"mail.send\" | \"calendar.write\" | \"contacts.write\" | \"tasks.write\" | \"hooks.arrival\" | \"hooks.beforeSend\" | \"hooks.open\" | \"composer\" | \"attachments\" | \"background\" | \"mcp.expose\" | `network:${string}`"
)]
pub enum PluginPermission {
    /// `mail.read`: accounts, folders and message headers (`nail.mail`),
    /// never bodies. Can be limited to accounts.
    MailRead,
    /// `mail.read.body`: message bodies too (needs `mail.read`). Never
    /// the decrypted content of end-to-end encrypted mail (that is
    /// `mail.read.encrypted`, M4). Can be limited to accounts.
    MailReadBody,
    /// `calendar.read`: the event occurrences of the user's calendars
    /// (`nail.calendar.events`; M3a, ADR 0063). Not limited to accounts.
    CalendarRead,
    /// `ui.dialogs`: open dialogs (`nail.ui.showDialog`).
    UiDialogs,
    /// `notifications`: in-app and OS notifications (`nail.ui.notify`).
    Notifications,
    /// `storage`: the plug-in's own key-value storage (`nail.storage`),
    /// within its quota.
    Storage,
    /// `mail.organize` (M4a): flag, tag, mark read, move, archive, snooze
    /// and delete messages (`nail.mail.update`, `nail.mail.delete`; Trash
    /// and permanent delete always ask the user). Needs `mail.read`. Can
    /// be limited to accounts.
    MailOrganize,
    /// `mail.send` (M4a): create drafts and send them
    /// (`nail.mail.createDraft`, `nail.mail.send`; sending asks the user
    /// unless they allow it). Can be limited to accounts (of the sending
    /// identity).
    MailSend,
    /// `calendar.write` (M4a): create, change and delete events
    /// (`nail.calendar.*`; mail to attendees also needs `mail.send`).
    /// Includes listing the calendars it may write to.
    CalendarWrite,
    /// `contacts.write` (M4a): create, change and delete contacts
    /// (`nail.contacts.*`). Includes listing address books and finding a
    /// contact by address.
    ContactsWrite,
    /// `tasks.write` (M4a): create, change and delete tasks
    /// (`nail.tasks.*`). Includes listing task lists.
    TasksWrite,
    /// `hooks.arrival` (M4a): called for new mail matching its filter; may
    /// answer actions like a rule's (actions need `mail.organize`). Needs
    /// `mail.read`. Can be limited to accounts.
    HooksArrival,
    /// `hooks.beforeSend` (M4a): sees every outgoing message before it is
    /// queued; may add headers or content, or veto with a reason the user
    /// can override ("Send anyway"). Can be limited to accounts.
    HooksBeforeSend,
    /// `hooks.open` (M4a): called when the user opens a message; may add
    /// banners and badges (never rewrites the message). Needs `mail.read`.
    /// Can be limited to accounts.
    HooksOpen,
    /// `composer` (M4a): composer actions (insert / transform text in the
    /// open draft, with the selected text) and Markdown block renderers.
    Composer,
    /// `attachments` (M4a): attachment preview renderers and actions on
    /// attachments (they receive the attachment the user picked). Can be
    /// limited to accounts.
    Attachments,
    /// `background` (M4a): background jobs on a schedule or on events
    /// (`message.received`, ...), within a CPU quota.
    Background,
    /// `mcp.expose` (M4a): offer its tools to MCP clients, under the
    /// clients' grants.
    McpExpose,
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
    /// A permission of the design that this version does not offer yet
    /// (`mail.read.encrypted`, `contacts.read`, `tasks.read`,
    /// `ui.webview`).
    #[error("permission `{0}` is not available in this version")]
    NotInPreview(String),
    /// `network:` with an invalid host pattern.
    #[error("invalid network domain `{0}` (expected a host name or `*.` + a domain)")]
    InvalidDomain(String),
}

/// Permission names of the design that this version refuses with
/// [`PluginPermissionError::NotInPreview`] (M4a offers the rest; the write
/// permissions include the reads they need, ADR 0079).
pub const FUTURE_PERMISSIONS: &[&str] = &[
    "mail.read.encrypted",
    "contacts.read",
    "tasks.read",
    "ui.webview",
];

/// Prefix of network permissions.
pub const NETWORK_PREFIX: &str = "network:";

impl PluginPermission {
    /// The permissions without an argument, in display order.
    pub const FIXED: [PluginPermission; 18] = [
        Self::MailRead,
        Self::MailReadBody,
        Self::CalendarRead,
        Self::UiDialogs,
        Self::Notifications,
        Self::Storage,
        Self::MailOrganize,
        Self::MailSend,
        Self::CalendarWrite,
        Self::ContactsWrite,
        Self::TasksWrite,
        Self::HooksArrival,
        Self::HooksBeforeSend,
        Self::HooksOpen,
        Self::Composer,
        Self::Attachments,
        Self::Background,
        Self::McpExpose,
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
            "mail.organize" => Self::MailOrganize,
            "mail.send" => Self::MailSend,
            "calendar.write" => Self::CalendarWrite,
            "contacts.write" => Self::ContactsWrite,
            "tasks.write" => Self::TasksWrite,
            "hooks.arrival" => Self::HooksArrival,
            "hooks.beforeSend" => Self::HooksBeforeSend,
            "hooks.open" => Self::HooksOpen,
            "composer" => Self::Composer,
            "attachments" => Self::Attachments,
            "background" => Self::Background,
            "mcp.expose" => Self::McpExpose,
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
            Self::Network(domain) => format!("{NETWORK_PREFIX}{domain}"),
            fixed => fixed.fixed_name().to_owned(),
        }
    }

    fn fixed_name(&self) -> &'static str {
        match self {
            Self::MailRead => "mail.read",
            Self::MailReadBody => "mail.read.body",
            Self::CalendarRead => "calendar.read",
            Self::UiDialogs => "ui.dialogs",
            Self::Notifications => "notifications",
            Self::Storage => "storage",
            Self::MailOrganize => "mail.organize",
            Self::MailSend => "mail.send",
            Self::CalendarWrite => "calendar.write",
            Self::ContactsWrite => "contacts.write",
            Self::TasksWrite => "tasks.write",
            Self::HooksArrival => "hooks.arrival",
            Self::HooksBeforeSend => "hooks.beforeSend",
            Self::HooksOpen => "hooks.open",
            Self::Composer => "composer",
            Self::Attachments => "attachments",
            Self::Background => "background",
            Self::McpExpose => "mcp.expose",
            Self::Network(_) => NETWORK_PREFIX,
        }
    }

    /// The permissions this one needs in the same manifest (M4a; the
    /// manifest check reports a missing one).
    pub fn requires(&self) -> &'static [PluginPermission] {
        match self {
            Self::MailReadBody | Self::MailOrganize | Self::HooksArrival | Self::HooksOpen => {
                &[Self::MailRead]
            }
            _ => &[],
        }
    }

    /// Whether a grant of this permission can be limited to accounts
    /// ([`PluginGrant::accounts`]).
    pub fn takes_accounts(&self) -> bool {
        matches!(
            self,
            Self::MailRead
                | Self::MailReadBody
                | Self::MailOrganize
                | Self::MailSend
                | Self::HooksArrival
                | Self::HooksBeforeSend
                | Self::HooksOpen
                | Self::Attachments
        )
    }

    /// Whether the permission lets the plug-in change the user's data
    /// (the install dialog groups and highlights these).
    pub fn is_write(&self) -> bool {
        matches!(
            self,
            Self::MailOrganize
                | Self::MailSend
                | Self::CalendarWrite
                | Self::ContactsWrite
                | Self::TasksWrite
        )
    }

    /// Whether this permission is accepted but grants nothing yet. None
    /// since M3a (`calendar.read` became real); kept for later previews.
    pub fn is_placeholder(&self) -> bool {
        false
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
            Self::MailOrganize => "plugin-permission-mail-organize",
            Self::MailSend => "plugin-permission-mail-send",
            Self::CalendarWrite => "plugin-permission-calendar-write",
            Self::ContactsWrite => "plugin-permission-contacts-write",
            Self::TasksWrite => "plugin-permission-tasks-write",
            Self::HooksArrival => "plugin-permission-hooks-arrival",
            Self::HooksBeforeSend => "plugin-permission-hooks-before-send",
            Self::HooksOpen => "plugin-permission-hooks-open",
            Self::Composer => "plugin-permission-composer",
            Self::Attachments => "plugin-permission-attachments",
            Self::Background => "plugin-permission-background",
            Self::McpExpose => "plugin-permission-mcp-expose",
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
    // Lowercase only; `*.com` would cover a whole top-level domain.
    crate::hosts::is_dns_name(host)
        && !host.bytes().any(|b| b.is_ascii_uppercase())
        && (!wildcard || host.contains('.'))
}

/// A granted permission.
///
/// JSON: `{"permission": "mail.read", "accounts": ["work"]}`; `accounts`
/// is `null` (or absent) for every account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct PluginGrant {
    /// The permission (one the manifest declares).
    pub permission: PluginPermission,
    /// For permissions that [`PluginPermission::takes_accounts`] (the mail
    /// permissions, mail hooks and `attachments`): the accounts (real or
    /// virtual) it reaches; `null` = all. Must be `null` for every other
    /// permission.
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
            PluginPermission::parse("contacts.read"),
            Err(PluginPermissionError::NotInPreview("contacts.read".into()))
        );
        assert_eq!(
            PluginPermission::parse("mail.send"),
            Ok(PluginPermission::MailSend)
        );
        assert_eq!(
            PluginPermission::parse("everything"),
            Err(PluginPermissionError::Unknown("everything".into()))
        );
        assert!(serde_json::from_value::<PluginPermission>(json!("ui.webview")).is_err());
        assert!(serde_json::from_value::<PluginPermission>(json!("hooks.beforesend")).is_err());
        for name in FUTURE_PERMISSIONS {
            assert!(PluginPermission::parse(name).is_err(), "{name}");
        }
    }

    #[test]
    fn properties_and_labels() {
        assert!(PluginPermission::MailRead.takes_accounts());
        assert!(PluginPermission::MailReadBody.takes_accounts());
        assert!(!PluginPermission::Storage.takes_accounts());
        assert!(!PluginPermission::CalendarRead.is_placeholder());
        assert!(!PluginPermission::CalendarRead.takes_accounts());
        assert_eq!(
            PluginPermission::MailRead.label().id,
            "plugin-permission-mail-read"
        );
        let net = PluginPermission::Network("a.example".into()).label();
        assert_eq!(net.id, "plugin-permission-network");
        assert_eq!(net.args["domain"], "a.example".into());
        let ids = PluginPermission::label_ids();
        assert_eq!(ids.len(), 19);
        assert_eq!(
            PluginPermission::HooksBeforeSend.label().id,
            "plugin-permission-hooks-before-send"
        );
        assert_eq!(PluginPermission::HooksBeforeSend.name(), "hooks.beforeSend");
        assert!(PluginPermission::MailOrganize.takes_accounts());
        assert!(PluginPermission::HooksOpen.takes_accounts());
        assert!(!PluginPermission::Background.takes_accounts());
        assert!(PluginPermission::TasksWrite.is_write());
        assert!(!PluginPermission::HooksArrival.is_write());
        assert_eq!(
            PluginPermission::HooksArrival.requires(),
            [PluginPermission::MailRead]
        );
        assert_eq!(PluginPermission::MailSend.requires().len(), 0);
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
