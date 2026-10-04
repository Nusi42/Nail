//! Microsoft Exchange accounts (M3c, ADR 0072).
//!
//! An Exchange account (`exchange` node in `accounts.kdl`,
//! [`crate::accounts::IncomingServer::Exchange`]) carries mail, calendars,
//! contacts and tasks over one protocol, chosen per account
//! ([`ExchangeEngine`]):
//!
//! * **Microsoft Graph** for Exchange Online / Microsoft 365: OAuth2 through
//!   the Microsoft identity platform ([`GRAPH_SCOPES`]), delta queries per
//!   folder and per calendar / contact folder / To Do list, polled with an
//!   adaptive interval (Graph change notifications need a public webhook,
//!   which a desktop client cannot receive).
//! * **Exchange Web Services** for Exchange Server on-premises (2016, 2019,
//!   Subscription Edition): `SyncFolderHierarchy` / `SyncFolderItems` per
//!   folder, streaming notifications for push; Windows integrated sign-in
//!   (Negotiate / Kerberos / NTLM through SSPI,
//!   [`crate::accounts::AuthConfig::Windows`]), Basic over TLS
//!   ([`crate::accounts::AuthConfig::Password`]) or OAuth2 for hybrid
//!   setups.
//! * **IMAP + SMTP** stays possible as a fallback when the admin enabled
//!   them: that is an ordinary IMAP account (no calendar or contacts);
//!   account setup proposes it next to the Exchange engines.
//!
//! Microsoft retires EWS in Exchange Online (announced from 2026-10-01;
//! the maintainer re-checks the timeline): Exchange Online accounts use Graph,
//! and an EWS account whose host is Exchange Online gets
//! [`ExchangeWarning::EwsRetiredOnline`].
//!
//! # How Exchange maps to Nail
//!
//! * Messages are fetched as full MIME (Graph `$value`, EWS
//!   `MimeContent`), so parsing, crypto, search and MCP stay the same.
//!   Every message has its server id in `messages.remote_id`; Exchange
//!   specifics are [`ExchangeMessageInfo`] on
//!   [`crate::MessageSummary::exchange`].
//! * **Categories** are Nail **tags** named like the category; the
//!   account's master category list ([`ExchangeCategory`]) gives them
//!   colours ([`CategoryColor`]). A tag set on an Exchange message is
//!   written as a category (Exchange accepts categories that are not in
//!   the master list).
//! * **Focused Inbox** is [`InferenceClassification`] on
//!   [`ExchangeMessageInfo::inference`], a filterable property (query
//!   `focus:focused` / `focus:other`); moving a message between Focused
//!   and Other goes through the Exchange operation queue
//!   ([`crate::Request::ClassifyFocused`]).
//! * **Shared mailboxes and delegate access** are extra folder trees under
//!   the account ([`ExchangeMailbox`]; folder paths
//!   [`mailbox_folder_path`]) and extra calendars / address books / task
//!   lists of the account's Exchange PIM source
//!   ([`ExchangeCollectionInfo::mailbox`]).
//! * **Server-side inbox rules** ([`rules`]) replace Sieve for Exchange
//!   accounts: Nail rules are placed as Exchange inbox rules where the
//!   semantics match ([`crate::sieve::RulePlacement`]), the user's own
//!   server rules are listed and can be switched on / off or deleted.
//! * **Automatic replies** (out of office, [`oof`]) replace the Sieve
//!   vacation reply.
//! * **Rooms and room lists** ([`Room`], [`RoomList`]) feed the room
//!   finder; room availability comes from the account's free/busy
//!   ([`crate::scheduling::FreeBusySource::Exchange`]).
//! * Calendars, contacts and tasks (Microsoft To Do lists through Graph,
//!   the Tasks folder through EWS) live in one PIM source per account
//!   ([`crate::pim::source::PimSourceKind::Exchange`],
//!   [`exchange_pim_source_id`]); [`pim`] holds the provider-neutral
//!   Exchange item shapes the engines produce and the mapping layer
//!   converts.
//!
//! Every write goes through a queue: messages through the mail operation
//! queue (replayed as [`crate::engine::RemoteWrite`]s), PIM items through
//! the PIM queue, everything else (categories, automatic replies, rules,
//! Focused Inbox) through the per-account Exchange operation queue
//! (`exchange_ops` in the store).

pub mod engine;
pub mod oof;
pub mod pim;
pub mod rules;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::accounts::{AuthConfig, ServerEndpoint, TlsMode};
use crate::domain::FolderRole;
use crate::error::ErrorCode;
use crate::ids::{AccountId, AddressBookId, CalendarId, ExchangeCategoryId, FolderId, PimSourceId};
use crate::localized::LocalizedMessage;
use crate::scheduling::BusyPeriod;

pub use oof::{AutomaticReplies, AutomaticRepliesSettings, AutomaticRepliesStatus};
pub use rules::{ExchangeInboxRule, ExchangeRulesStatus};

/// The Microsoft Graph host (the default `host` of a Graph `exchange`
/// node).
pub const GRAPH_HOST: &str = "graph.microsoft.com";

/// The Graph API version path below the host
/// (`https://graph.microsoft.com/v1.0/me/…`).
pub const GRAPH_BASE_PATH: &str = "/v1.0";

/// Exchange Online's host (EWS, Autodiscover, IMAP fallback).
pub const EXCHANGE_ONLINE_HOST: &str = "outlook.office365.com";

/// The usual EWS endpoint path below the host.
pub const EWS_PATH: &str = "/EWS/Exchange.asmx";

/// The OAuth2 scopes Nail asks the Microsoft identity platform for when an
/// account uses Graph: mail (incl. shared mailboxes), sending, calendars,
/// contacts, To Do, mailbox settings (automatic replies, inbox rules,
/// categories), rooms (`Place.Read.All` needs admin consent in many
/// tenants; without it room lists answer `forbidden`), and a refresh
/// token.
pub const GRAPH_SCOPES: [&str; 14] = [
    "https://graph.microsoft.com/User.Read",
    "https://graph.microsoft.com/Mail.ReadWrite",
    "https://graph.microsoft.com/Mail.ReadWrite.Shared",
    "https://graph.microsoft.com/Mail.Send",
    "https://graph.microsoft.com/Mail.Send.Shared",
    "https://graph.microsoft.com/MailboxSettings.ReadWrite",
    "https://graph.microsoft.com/Calendars.ReadWrite",
    "https://graph.microsoft.com/Calendars.ReadWrite.Shared",
    "https://graph.microsoft.com/Contacts.ReadWrite",
    "https://graph.microsoft.com/Contacts.ReadWrite.Shared",
    "https://graph.microsoft.com/Tasks.ReadWrite",
    "https://graph.microsoft.com/Place.Read.All",
    "https://graph.microsoft.com/User.ReadBasic.All",
    "offline_access",
];

/// The OAuth2 scope for EWS against Exchange Online / hybrid (plus
/// `offline_access`).
pub const EWS_OAUTH_SCOPE: &str = "https://outlook.office365.com/EWS.AccessAsUser.All";

/// Longest category name Exchange accepts (characters).
pub const MAX_CATEGORY_NAME: usize = 255;

/// Prefix of the temporary id of a category created locally (until the
/// server assigned one).
pub const LOCAL_CATEGORY_PREFIX: &str = "local-";

/// Which protocol an Exchange account speaks.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeEngine {
    /// Microsoft Graph (Exchange Online / Microsoft 365).
    #[default]
    Graph,
    /// Exchange Web Services (Exchange Server on-premises).
    Ews,
}

impl ExchangeEngine {
    /// Both engines.
    pub const ALL: [ExchangeEngine; 2] = [Self::Graph, Self::Ews];

    /// The name in `accounts.kdl` (`engine="graph"`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Graph => "graph",
            Self::Ews => "ews",
        }
    }

    /// The engine of a name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|e| e.as_str() == name)
    }

    /// The Fluent id of the engine's name (`exchange-engine-graph`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Graph => "exchange-engine-graph",
            Self::Ews => "exchange-engine-ews",
        }
    }
}

/// Account settings of an `exchange` node (`accounts.kdl`). `null` = the
/// documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ExchangeOptions {
    /// The protocol (default Graph).
    pub engine: ExchangeEngine,
    /// EWS only: the full endpoint URL Autodiscover found, `null` =
    /// `https://<host>/EWS/Exchange.asmx` ([`Self::ews_url`]).
    pub url: Option<String>,
    /// Shortest polling interval in seconds (Graph deltas; EWS without
    /// streaming). Default 30, at least 10.
    pub poll_interval_secs: Option<u32>,
    /// Longest polling interval in seconds. Default 300.
    pub max_poll_interval_secs: Option<u32>,
    /// EWS only: use streaming notifications for push. Default true.
    pub streaming: Option<bool>,
    /// Place Nail rules as Exchange inbox rules where they are
    /// expressible. Default true; `false` runs every rule in Nail.
    pub server_rules: Option<bool>,
}

impl ExchangeOptions {
    /// Default of [`Self::poll_interval_secs`].
    pub const DEFAULT_POLL_INTERVAL_SECS: u32 = 30;
    /// Default of [`Self::max_poll_interval_secs`].
    pub const DEFAULT_MAX_POLL_INTERVAL_SECS: u32 = 300;
    /// Smallest accepted [`Self::poll_interval_secs`] (throttling friendly).
    pub const MIN_POLL_INTERVAL_SECS: u32 = 10;

    /// Options for an engine with every default.
    pub fn for_engine(engine: ExchangeEngine) -> Self {
        Self {
            engine,
            ..Self::default()
        }
    }

    /// The effective (minimum, maximum) polling interval in seconds.
    pub fn poll_range(&self) -> (u32, u32) {
        let min = self
            .poll_interval_secs
            .unwrap_or(Self::DEFAULT_POLL_INTERVAL_SECS)
            .max(Self::MIN_POLL_INTERVAL_SECS);
        let max = self
            .max_poll_interval_secs
            .unwrap_or(Self::DEFAULT_MAX_POLL_INTERVAL_SECS)
            .max(min);
        (min, max)
    }

    /// Whether EWS streaming notifications are used.
    pub fn streaming(&self) -> bool {
        self.engine == ExchangeEngine::Ews && self.streaming.unwrap_or(true)
    }

    /// Whether Nail rules are placed as server rules.
    pub fn server_rules(&self) -> bool {
        self.server_rules.unwrap_or(true)
    }

    /// The EWS endpoint: the configured URL, else
    /// `https://<host>[:port]/EWS/Exchange.asmx` (`http` for plaintext
    /// loopback test servers).
    pub fn ews_url(&self, server: &ServerEndpoint) -> String {
        if let Some(url) = &self.url {
            return url.clone();
        }
        let tls = server.tls != TlsMode::Plaintext;
        base_url(&server.host, server.port, tls, EWS_PATH)
    }

    /// The Graph base URL (`https://graph.microsoft.com/v1.0`).
    pub fn graph_url(&self, server: &ServerEndpoint) -> String {
        let tls = server.tls != TlsMode::Plaintext;
        base_url(&server.host, server.port, tls, GRAPH_BASE_PATH)
    }

    /// Problems of these options with `server` (empty = valid): the rules
    /// the `accounts.kdl` reader enforces (Fluent ids
    /// [`ExchangeConfigIssue::message_id`]).
    pub fn issues(&self, server: &ServerEndpoint) -> Vec<ExchangeConfigIssue> {
        let mut out = Vec::new();
        match (self.engine, &server.auth) {
            (ExchangeEngine::Graph, AuthConfig::OAuth2 { .. })
            | (
                ExchangeEngine::Ews,
                AuthConfig::OAuth2 { .. }
                | AuthConfig::Password { .. }
                | AuthConfig::Windows { .. },
            ) => {}
            (ExchangeEngine::Graph, _) => out.push(ExchangeConfigIssue::GraphNeedsOAuth2),
            (ExchangeEngine::Ews, _) => out.push(ExchangeConfigIssue::UnsupportedAuth),
        }
        if server.tls == TlsMode::Plaintext && !is_loopback_host(&server.host) {
            out.push(ExchangeConfigIssue::PlaintextNeedsLoopback);
        }
        if server.tls == TlsMode::Starttls {
            out.push(ExchangeConfigIssue::Starttls);
        }
        if self.engine == ExchangeEngine::Graph && (self.url.is_some() || self.streaming.is_some())
        {
            out.push(ExchangeConfigIssue::EwsOnlyOption);
        }
        if let Some(url) = &self.url
            && !crate::pim::is_https_or_loopback_url(url)
        {
            out.push(ExchangeConfigIssue::InvalidUrl);
        }
        out
    }

    /// Warnings worth showing for a valid configuration.
    pub fn warnings(&self, server: &ServerEndpoint) -> Vec<ExchangeWarning> {
        let mut out = Vec::new();
        let host = self
            .url
            .as_deref()
            .and_then(url_host)
            .unwrap_or(server.host.as_str())
            .to_ascii_lowercase();
        if self.engine == ExchangeEngine::Ews && is_exchange_online_host(&host) {
            out.push(ExchangeWarning::EwsRetiredOnline);
        }
        out
    }
}

fn base_url(host: &str, port: u16, tls: bool, path: &str) -> String {
    let scheme = if tls { "https" } else { "http" };
    let default_port = if tls { 443 } else { 80 };
    if port == default_port {
        format!("{scheme}://{host}{path}")
    } else {
        format!("{scheme}://{host}:{port}{path}")
    }
}

fn url_host(url: &str) -> Option<&str> {
    let rest = url.split_once("://")?.1;
    let authority = rest.split(['/', '?', '#']).next()?;
    let host = authority.rsplit_once('@').map_or(authority, |(_, h)| h);
    Some(host.split(':').next().unwrap_or(host))
}

/// Whether `host` is a loopback name or address (test servers).
pub fn is_loopback_host(host: &str) -> bool {
    let host = host.trim_start_matches('[').trim_end_matches(']');
    host.eq_ignore_ascii_case("localhost")
        || host
            .parse::<std::net::IpAddr>()
            .is_ok_and(|ip| ip.is_loopback())
}

/// Whether `host` is Exchange Online (`outlook.office365.com`,
/// `outlook.office.com`, `*.outlook.com`).
pub fn is_exchange_online_host(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let under = |domain: &str| host == domain || host.ends_with(&format!(".{domain}"));
    under("office365.com") || under("outlook.office.com") || under("outlook.com")
}

/// A problem with an `exchange` node.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeConfigIssue {
    /// Graph needs `auth "oauth2"`.
    GraphNeedsOAuth2,
    /// EWS takes `oauth2`, `password` (Basic over TLS) or `windows`.
    UnsupportedAuth,
    /// `tls="plaintext"` outside loopback test servers.
    PlaintextNeedsLoopback,
    /// HTTPS has no STARTTLS.
    Starttls,
    /// `url` / `streaming` on a Graph account.
    EwsOnlyOption,
    /// `url` is not `https:` (or `http:` on a loopback host).
    InvalidUrl,
}

impl ExchangeConfigIssue {
    /// Every issue.
    pub const ALL: [ExchangeConfigIssue; 6] = [
        Self::GraphNeedsOAuth2,
        Self::UnsupportedAuth,
        Self::PlaintextNeedsLoopback,
        Self::Starttls,
        Self::EwsOnlyOption,
        Self::InvalidUrl,
    ];

    /// The Fluent id (`config-exchange-*`, in `core.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::GraphNeedsOAuth2 => "config-exchange-graph-needs-oauth2",
            Self::UnsupportedAuth => "config-exchange-unsupported-auth",
            Self::PlaintextNeedsLoopback => "config-exchange-plaintext-needs-loopback",
            Self::Starttls => "config-exchange-starttls",
            Self::EwsOnlyOption => "config-exchange-ews-only-option",
            Self::InvalidUrl => "config-exchange-invalid-url",
        }
    }
}

/// Something about an Exchange account the user should know.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeWarning {
    /// EWS against Exchange Online, which Microsoft retires: switch the
    /// account to Graph.
    EwsRetiredOnline,
    /// The server does not classify mail for Focused Inbox (EWS, older
    /// servers, or switched off by the admin): no Focused / Other split.
    FocusedInboxUnavailable,
    /// Rooms need the `Place.Read.All` permission (admin consent), which
    /// the tenant did not grant.
    RoomsNeedConsent,
    /// The server throttles Nail; syncing slows down until
    /// [`ExchangeStatus::throttled_until`].
    Throttled,
}

impl ExchangeWarning {
    /// Every warning.
    pub const ALL: [ExchangeWarning; 4] = [
        Self::EwsRetiredOnline,
        Self::FocusedInboxUnavailable,
        Self::RoomsNeedConsent,
        Self::Throttled,
    ];

    /// The Fluent id (`exchange-warning-*`, in `core-exchange.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::EwsRetiredOnline => "exchange-warning-ews-retired-online",
            Self::FocusedInboxUnavailable => "exchange-warning-focused-inbox-unavailable",
            Self::RoomsNeedConsent => "exchange-warning-rooms-need-consent",
            Self::Throttled => "exchange-warning-throttled",
        }
    }
}

// ---------------------------------------------------------------------------
// Messages and folders

/// Focused Inbox: the server's classification of an inbox message
/// (Graph `inferenceClassification`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum InferenceClassification {
    /// "Focused".
    Focused,
    /// "Other".
    Other,
}

impl InferenceClassification {
    /// Both values.
    pub const ALL: [InferenceClassification; 2] = [Self::Focused, Self::Other];

    /// The wire / query name (`focused`, `other`), as used by the query
    /// language's `focus:`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Focused => "focused",
            Self::Other => "other",
        }
    }

    /// The value of a query name (case-insensitive).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(name))
    }

    /// The Fluent id of the tab name (`exchange-focus-focused`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::Focused => "exchange-focus-focused",
            Self::Other => "exchange-focus-other",
        }
    }
}

/// Exchange-specific data of a message ([`crate::MessageSummary::exchange`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeMessageInfo {
    /// The server's item id (also `messages.remote_id`).
    pub item_id: String,
    /// The conversation it belongs to (Graph `conversationId`, EWS
    /// `ConversationId`), if any.
    #[serde(default)]
    pub conversation_id: Option<String>,
    /// Focused Inbox classification (inbox messages of servers that
    /// classify; absent elsewhere).
    #[serde(default)]
    pub inference: Option<InferenceClassification>,
    /// The shared / delegate mailbox the message is in (lowercase
    /// address); `null` = the account's own mailbox.
    #[serde(default)]
    pub mailbox: Option<String>,
}

/// A well-known folder of a mailbox (Graph well-known folder names, EWS
/// distinguished folder ids).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeWellKnownFolder {
    /// The mailbox root (`msgfolderroot`); not shown.
    Root,
    /// `inbox`.
    Inbox,
    /// `sentitems`.
    SentItems,
    /// `drafts`.
    Drafts,
    /// `deleteditems`.
    DeletedItems,
    /// `junkemail`.
    JunkEmail,
    /// `archive` (Graph; EWS finds it by its folder class / name).
    Archive,
    /// `outbox`; not shown (Nail has its own outbox).
    Outbox,
    /// `calendar`.
    Calendar,
    /// `contacts`.
    Contacts,
    /// `tasks`.
    Tasks,
}

impl ExchangeWellKnownFolder {
    /// Every well-known folder.
    pub const ALL: [ExchangeWellKnownFolder; 11] = [
        Self::Root,
        Self::Inbox,
        Self::SentItems,
        Self::Drafts,
        Self::DeletedItems,
        Self::JunkEmail,
        Self::Archive,
        Self::Outbox,
        Self::Calendar,
        Self::Contacts,
        Self::Tasks,
    ];

    /// The Graph well-known folder name (`/me/mailFolders/{name}`), for
    /// mail folders; `None` for PIM folders (Graph has their own
    /// endpoints).
    pub fn graph_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Root => "msgfolderroot",
            Self::Inbox => "inbox",
            Self::SentItems => "sentitems",
            Self::Drafts => "drafts",
            Self::DeletedItems => "deleteditems",
            Self::JunkEmail => "junkemail",
            Self::Archive => "archive",
            Self::Outbox => "outbox",
            Self::Calendar | Self::Contacts | Self::Tasks => return None,
        })
    }

    /// The EWS `DistinguishedFolderId`, where EWS has one.
    pub fn ews_name(self) -> Option<&'static str> {
        Some(match self {
            Self::Root => "msgfolderroot",
            Self::Inbox => "inbox",
            Self::SentItems => "sentitems",
            Self::Drafts => "drafts",
            Self::DeletedItems => "deleteditems",
            Self::JunkEmail => "junkemail",
            Self::Outbox => "outbox",
            Self::Calendar => "calendar",
            Self::Contacts => "contacts",
            Self::Tasks => "tasks",
            Self::Archive => return None,
        })
    }

    /// The well-known folder of a Graph / EWS name (case-insensitive).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|f| {
            f.graph_name().is_some_and(|n| n.eq_ignore_ascii_case(name))
                || f.ews_name().is_some_and(|n| n.eq_ignore_ascii_case(name))
        })
    }

    /// The special-use role of the folder in Nail's folder list; `None`
    /// for folders Nail does not show as mail folders (root, outbox, PIM
    /// folders).
    pub fn role(self) -> Option<FolderRole> {
        match self {
            Self::Inbox => Some(FolderRole::Inbox),
            Self::SentItems => Some(FolderRole::Sent),
            Self::Drafts => Some(FolderRole::Drafts),
            Self::DeletedItems => Some(FolderRole::Trash),
            Self::JunkEmail => Some(FolderRole::Junk),
            Self::Archive => Some(FolderRole::Archive),
            Self::Root | Self::Outbox | Self::Calendar | Self::Contacts | Self::Tasks => None,
        }
    }
}

/// The store path of an Exchange folder: the display path (`Inbox/Receipts`)
/// for the account's own mailbox; `@<mailbox>/<display path>` for a shared
/// or delegate mailbox, whose root folder is `@<mailbox>`. Paths are for
/// display and folder policies only — the store keys Exchange folders by
/// their remote id (a clash with a user folder named `@…` gets a suffix).
pub fn mailbox_folder_path(mailbox: Option<&str>, display_path: &str) -> String {
    match mailbox {
        None => display_path.to_owned(),
        Some(mailbox) if display_path.is_empty() => format!("@{}", mailbox.to_ascii_lowercase()),
        Some(mailbox) => format!("@{}/{display_path}", mailbox.to_ascii_lowercase()),
    }
}

/// The mailbox and display path of a store path written by
/// [`mailbox_folder_path`] (`None` mailbox = the account's own).
pub fn split_mailbox_folder_path(path: &str) -> (Option<&str>, &str) {
    match path.strip_prefix('@') {
        Some(rest) if rest.contains('@') => match rest.split_once('/') {
            Some((mailbox, display)) => (Some(mailbox), display),
            None => (Some(rest), ""),
        },
        _ => (None, path),
    }
}

/// The id of the Exchange PIM source of an account: `exchange:<account>`
/// (a `:` never appears in configured source names, so it cannot clash).
pub fn exchange_pim_source_id(account: &AccountId) -> PimSourceId {
    PimSourceId::new(format!("exchange:{account}"))
}

/// The account of an Exchange PIM source id, if it is one.
pub fn exchange_pim_source_account(source: &PimSourceId) -> Option<AccountId> {
    source
        .as_str()
        .strip_prefix("exchange:")
        .filter(|a| !a.is_empty())
        .map(AccountId::new)
}

// ---------------------------------------------------------------------------
// Shared mailboxes and delegate access

/// How the user reaches another mailbox.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeMailboxKind {
    /// A shared mailbox (full access).
    Shared,
    /// Another user's mailbox the user was given delegate access to
    /// (usually some folders, e.g. the calendar).
    Delegate,
}

/// Where a mailbox entry came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeMailboxOrigin {
    /// Added by the user.
    #[default]
    User,
    /// Found by Autodiscover (Exchange "automapping"); removing it hides
    /// it instead of deleting it.
    Automapped,
}

/// What of another mailbox is shown.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[expect(
    clippy::struct_excessive_bools,
    reason = "independent per-mailbox switches (one checkbox each)"
)]
pub struct ExchangeMailboxSettings {
    /// The mailbox's address.
    pub address: String,
    /// Display name, `null` = the server's.
    #[serde(default)]
    pub name: Option<String>,
    /// Shared or delegate.
    pub kind: ExchangeMailboxKind,
    /// Show its mail folders (a folder tree under the account).
    #[serde(default = "yes")]
    pub mail: bool,
    /// Show its calendar.
    #[serde(default = "yes")]
    pub calendar: bool,
    /// Show its contacts.
    #[serde(default)]
    pub contacts: bool,
    /// Show its tasks.
    #[serde(default)]
    pub tasks: bool,
}

fn yes() -> bool {
    true
}

impl ExchangeMailboxSettings {
    /// A shared mailbox with mail and calendar.
    pub fn shared(address: impl Into<String>) -> Self {
        Self {
            address: address.into(),
            name: None,
            kind: ExchangeMailboxKind::Shared,
            mail: true,
            calendar: true,
            contacts: false,
            tasks: false,
        }
    }

    /// Problems (empty = valid). `own` is the account's own address.
    pub fn issues(&self, own: &str) -> Vec<ExchangeMailboxIssue> {
        let mut out = Vec::new();
        let address = self.address.trim();
        if address.is_empty() {
            out.push(ExchangeMailboxIssue::MissingAddress);
        } else if !crate::pim::is_plausible_email(address) {
            out.push(ExchangeMailboxIssue::InvalidAddress);
        } else if address.eq_ignore_ascii_case(own.trim()) {
            out.push(ExchangeMailboxIssue::OwnMailbox);
        }
        if self.name.as_deref().is_some_and(|n| n.trim().is_empty()) {
            out.push(ExchangeMailboxIssue::EmptyName);
        }
        if !(self.mail || self.calendar || self.contacts || self.tasks) {
            out.push(ExchangeMailboxIssue::NothingShown);
        }
        out
    }
}

/// A problem with [`ExchangeMailboxSettings`]; the core answers
/// `invalid_request` with [`Self::message_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeMailboxIssue {
    /// No address.
    MissingAddress,
    /// Not an email address.
    InvalidAddress,
    /// The account's own address.
    OwnMailbox,
    /// The mailbox is already listed.
    Duplicate,
    /// A blank name.
    EmptyName,
    /// Neither mail, calendar, contacts nor tasks are shown.
    NothingShown,
}

impl ExchangeMailboxIssue {
    /// Every issue.
    pub const ALL: [ExchangeMailboxIssue; 6] = [
        Self::MissingAddress,
        Self::InvalidAddress,
        Self::OwnMailbox,
        Self::Duplicate,
        Self::EmptyName,
        Self::NothingShown,
    ];

    /// The Fluent id (`exchange-mailbox-issue-*`, in `core-exchange.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::MissingAddress => "exchange-mailbox-issue-missing-address",
            Self::InvalidAddress => "exchange-mailbox-issue-invalid-address",
            Self::OwnMailbox => "exchange-mailbox-issue-own-mailbox",
            Self::Duplicate => "exchange-mailbox-issue-duplicate",
            Self::EmptyName => "exchange-mailbox-issue-empty-name",
            Self::NothingShown => "exchange-mailbox-issue-nothing-shown",
        }
    }
}

/// Whether another mailbox can be reached.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeMailboxState {
    /// Added, not synced yet.
    #[default]
    Pending,
    /// Synced.
    Synced,
    /// The server refused access (no permission on the mailbox or the
    /// chosen folders).
    AccessDenied,
    /// No such mailbox.
    NotFound,
    /// The last sync failed for another reason.
    Error,
}

impl ExchangeMailboxState {
    /// Every state.
    pub const ALL: [ExchangeMailboxState; 5] = [
        Self::Pending,
        Self::Synced,
        Self::AccessDenied,
        Self::NotFound,
        Self::Error,
    ];

    /// The wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Synced => "synced",
            Self::AccessDenied => "access-denied",
            Self::NotFound => "not-found",
            Self::Error => "error",
        }
    }

    /// The state of a wire name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|s| s.as_str() == name)
    }

    /// The Fluent id (`exchange-mailbox-state-*`).
    pub fn message_id(self) -> String {
        format!("exchange-mailbox-state-{}", self.as_str())
    }
}

/// Another mailbox of an Exchange account
/// ([`crate::Request::ListExchangeMailboxes`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeMailbox {
    /// The account.
    pub account_id: AccountId,
    /// What is shown.
    pub settings: ExchangeMailboxSettings,
    /// Added by the user or automapped.
    pub origin: ExchangeMailboxOrigin,
    /// Reachability.
    pub state: ExchangeMailboxState,
    /// The root of its folder tree under the account (once synced).
    #[serde(default)]
    pub root_folder_id: Option<FolderId>,
    /// Its calendars / task lists shown.
    #[serde(default)]
    pub calendar_ids: Vec<CalendarId>,
    /// Its address books shown.
    #[serde(default)]
    pub address_book_ids: Vec<AddressBookId>,
    /// Error class of the last failure.
    #[serde(default)]
    pub error: Option<ErrorCode>,
    /// The user-facing text of the failure (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

// ---------------------------------------------------------------------------
// Categories

/// An Outlook category colour (`preset0` … `preset24`, or none).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum CategoryColor {
    /// No colour.
    #[default]
    None,
    /// Red.
    Preset0,
    /// Orange.
    Preset1,
    /// Brown.
    Preset2,
    /// Yellow.
    Preset3,
    /// Green.
    Preset4,
    /// Teal.
    Preset5,
    /// Olive.
    Preset6,
    /// Blue.
    Preset7,
    /// Purple.
    Preset8,
    /// Cranberry.
    Preset9,
    /// Steel.
    Preset10,
    /// Dark steel.
    Preset11,
    /// Gray.
    Preset12,
    /// Dark gray.
    Preset13,
    /// Black.
    Preset14,
    /// Dark red.
    Preset15,
    /// Dark orange.
    Preset16,
    /// Dark brown.
    Preset17,
    /// Dark yellow.
    Preset18,
    /// Dark green.
    Preset19,
    /// Dark teal.
    Preset20,
    /// Dark olive.
    Preset21,
    /// Dark blue.
    Preset22,
    /// Dark purple.
    Preset23,
    /// Dark cranberry.
    Preset24,
}

impl CategoryColor {
    /// Every colour, `none` first, then the presets in order.
    pub const ALL: [CategoryColor; 26] = [
        Self::None,
        Self::Preset0,
        Self::Preset1,
        Self::Preset2,
        Self::Preset3,
        Self::Preset4,
        Self::Preset5,
        Self::Preset6,
        Self::Preset7,
        Self::Preset8,
        Self::Preset9,
        Self::Preset10,
        Self::Preset11,
        Self::Preset12,
        Self::Preset13,
        Self::Preset14,
        Self::Preset15,
        Self::Preset16,
        Self::Preset17,
        Self::Preset18,
        Self::Preset19,
        Self::Preset20,
        Self::Preset21,
        Self::Preset22,
        Self::Preset23,
        Self::Preset24,
    ];

    /// Graph's name (`none`, `preset0` … `preset24`).
    pub fn as_str(self) -> &'static str {
        const NAMES: [&str; 26] = [
            "none", "preset0", "preset1", "preset2", "preset3", "preset4", "preset5", "preset6",
            "preset7", "preset8", "preset9", "preset10", "preset11", "preset12", "preset13",
            "preset14", "preset15", "preset16", "preset17", "preset18", "preset19", "preset20",
            "preset21", "preset22", "preset23", "preset24",
        ];
        NAMES[self.position()]
    }

    fn position(self) -> usize {
        Self::ALL.iter().position(|c| *c == self).unwrap_or(0)
    }

    /// The colour of Graph's name (case-insensitive).
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL
            .into_iter()
            .find(|c| c.as_str().eq_ignore_ascii_case(name))
    }

    /// The EWS / Outlook colour index (`-1` = none, `0` … `24`).
    pub fn ews_index(self) -> i32 {
        i32::try_from(self.position()).unwrap_or(0) - 1
    }

    /// The colour of an EWS / Outlook index (out of range = none).
    pub fn from_ews_index(index: i32) -> Self {
        usize::try_from(index + 1)
            .ok()
            .and_then(|i| Self::ALL.get(i).copied())
            .unwrap_or(Self::None)
    }

    /// The colour Nail draws (`#rrggbb`, Outlook's palette); `None` for
    /// no colour.
    pub fn rgb(self) -> Option<&'static str> {
        const RGB: [&str; 25] = [
            "#e7a1a2", "#f9ba89", "#f7dd8f", "#fcfa90", "#78d168", "#9fdcc9", "#c6d2b0", "#9db7e8",
            "#b5a1e2", "#daaec2", "#dad9dc", "#6b7994", "#bfbfbf", "#6f6f6f", "#4f4f4f", "#c11a25",
            "#e2620d", "#c79930", "#b9b300", "#368f2b", "#329b7a", "#778b45", "#2858a5", "#5c3fa3",
            "#93446b",
        ];
        self.position().checked_sub(1).map(|i| RGB[i])
    }

    /// The Fluent id of the colour's name (`exchange-color-preset0`).
    pub fn message_id(self) -> String {
        format!("exchange-color-{}", self.as_str())
    }
}

/// The editable settings of a category.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeCategorySettings {
    /// Name; also the Nail tag.
    pub name: String,
    /// Colour.
    #[serde(default)]
    pub color: CategoryColor,
}

impl ExchangeCategorySettings {
    /// Settings with a name and no colour.
    pub fn named(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            color: CategoryColor::None,
        }
    }

    /// Problems (empty = valid; duplicates are checked by the core).
    pub fn issues(&self) -> Vec<ExchangeCategoryIssue> {
        let mut out = Vec::new();
        let name = self.name.trim();
        if name.is_empty() {
            out.push(ExchangeCategoryIssue::MissingName);
        } else {
            if name.chars().count() > MAX_CATEGORY_NAME {
                out.push(ExchangeCategoryIssue::NameTooLong);
            }
            // Outlook separates categories with commas / semicolons.
            if name.contains([',', ';']) || name.chars().any(char::is_control) {
                out.push(ExchangeCategoryIssue::InvalidName);
            }
        }
        out
    }
}

/// A problem with [`ExchangeCategorySettings`]; the core answers
/// `invalid_request` with [`Self::message_id`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeCategoryIssue {
    /// The name is empty.
    MissingName,
    /// Longer than [`MAX_CATEGORY_NAME`].
    NameTooLong,
    /// A comma, semicolon or control character.
    InvalidName,
    /// Another category of the account has this name (case-insensitive).
    DuplicateName,
}

impl ExchangeCategoryIssue {
    /// Every issue.
    pub const ALL: [ExchangeCategoryIssue; 4] = [
        Self::MissingName,
        Self::NameTooLong,
        Self::InvalidName,
        Self::DuplicateName,
    ];

    /// The Fluent id (`exchange-category-issue-*`, in `core-exchange.ftl`).
    pub fn message_id(self) -> &'static str {
        match self {
            Self::MissingName => "exchange-category-issue-missing-name",
            Self::NameTooLong => "exchange-category-issue-name-too-long",
            Self::InvalidName => "exchange-category-issue-invalid-name",
            Self::DuplicateName => "exchange-category-issue-duplicate-name",
        }
    }
}

/// Local sync state of something queued in the Exchange operation queue.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeSync {
    /// As on the server.
    #[default]
    Synced,
    /// Changed locally; the operation is queued.
    Pending,
}

impl ExchangeSync {
    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Synced => "synced",
            Self::Pending => "pending",
        }
    }

    /// The state of a name (`pending`, else synced).
    pub fn parse(name: &str) -> Self {
        if name == "pending" {
            Self::Pending
        } else {
            Self::Synced
        }
    }
}

/// A category of the account's master list
/// ([`crate::Request::ListExchangeCategories`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeCategory {
    /// The account.
    pub account_id: AccountId,
    /// The server's id (a `local-…` id while its creation is queued).
    pub id: ExchangeCategoryId,
    /// Name = the Nail tag.
    pub name: String,
    /// Colour.
    pub color: CategoryColor,
    /// Messages with the tag (as stored locally).
    pub total: u32,
    /// Unread ones among them.
    pub unread: u32,
    /// Sync state.
    #[serde(default)]
    pub sync: ExchangeSync,
}

impl ExchangeCategory {
    /// The editable settings.
    pub fn settings(&self) -> ExchangeCategorySettings {
        ExchangeCategorySettings {
            name: self.name.clone(),
            color: self.color,
        }
    }
}

// ---------------------------------------------------------------------------
// Rooms

/// A room list (a distribution list of rooms, e.g. per building).
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct RoomList {
    /// Its address.
    pub address: String,
    /// Display name.
    pub name: String,
}

/// A meeting room (Graph `place` / EWS room).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Room {
    /// Its address (the attendee address that books it).
    pub address: String,
    /// Display name.
    pub name: String,
    /// Seats, if known.
    #[serde(default)]
    pub capacity: Option<u32>,
    /// Building, if known.
    #[serde(default)]
    pub building: Option<String>,
    /// Floor label, if known.
    #[serde(default)]
    pub floor: Option<String>,
    /// Wheelchair accessible, if known.
    #[serde(default)]
    pub accessible: Option<bool>,
    /// Equipment and tags the server lists (`display`, `video`, …), as
    /// the server words them.
    #[serde(default)]
    pub features: Vec<String>,
    /// The room list it came from, if any.
    #[serde(default)]
    pub room_list: Option<String>,
}

/// A room with its availability in a range ([`crate::Request::FindRooms`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct RoomAvailability {
    /// The room.
    pub room: Room,
    /// Free for the whole range (`null` when unknown).
    #[serde(default)]
    pub free: Option<bool>,
    /// Busy periods in the range (merged, sorted).
    #[serde(default)]
    pub periods: Vec<BusyPeriod>,
    /// Asking failed for this room.
    #[serde(default)]
    pub error: Option<ErrorCode>,
}

// ---------------------------------------------------------------------------
// PIM collections

/// The special role of an Exchange calendar / address book / task list.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeCollectionRole {
    /// The mailbox's default calendar.
    DefaultCalendar,
    /// The default contacts folder.
    DefaultContacts,
    /// The default task list (To Do "Tasks", EWS Tasks folder).
    DefaultTasks,
    /// To Do's "Flagged email" list (flagged messages as tasks; Graph).
    FlaggedEmails,
}

impl ExchangeCollectionRole {
    /// Every role.
    pub const ALL: [ExchangeCollectionRole; 4] = [
        Self::DefaultCalendar,
        Self::DefaultContacts,
        Self::DefaultTasks,
        Self::FlaggedEmails,
    ];

    /// The wire / store name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::DefaultCalendar => "default-calendar",
            Self::DefaultContacts => "default-contacts",
            Self::DefaultTasks => "default-tasks",
            Self::FlaggedEmails => "flagged-emails",
        }
    }

    /// The role of a name.
    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == name)
    }
}

/// Exchange data of a calendar, task list or address book
/// ([`crate::pim::Calendar::exchange`], [`crate::pim::AddressBook::exchange`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeCollectionInfo {
    /// The shared / delegate mailbox it belongs to (lowercase); `null` =
    /// the account's own.
    #[serde(default)]
    pub mailbox: Option<String>,
    /// A special role.
    #[serde(default)]
    pub role: Option<ExchangeCollectionRole>,
    /// A Microsoft To Do list (Graph `todoTaskList`) rather than an EWS
    /// task folder.
    #[serde(default)]
    pub todo: bool,
}

// ---------------------------------------------------------------------------
// Account setup (WP95)

/// How Autodiscover found a host that would get the login
/// ([`ExchangeDiscoveryPrompt::ConfirmRedirect`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeRedirectReason {
    /// An HTTP redirect to another domain.
    HttpRedirect,
    /// A POX `redirectUrl` to another domain.
    RedirectUrl,
    /// A POX `redirectAddr` to an address of another domain.
    RedirectAddr,
    /// The unauthenticated HTTP redirect method.
    HttpMethod,
    /// A DNS SRV record (`_autodiscover._tcp`).
    Srv,
}

/// Autodiscover stopped to ask the user ([`crate::Request::DiscoverAccount`]).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum ExchangeDiscoveryPrompt {
    /// The next step would send the login to `host`, outside the address's
    /// domain: ask again with the host in `trusted_hosts` once the user
    /// trusts it (`exchange-autodiscover-confirm-redirect`).
    ConfirmRedirect {
        /// The URL that would get the login.
        url: String,
        /// Its host.
        host: String,
        /// The address's domain.
        domain: String,
        /// How the host was found.
        reason: ExchangeRedirectReason,
        /// The question to show.
        localized: LocalizedMessage,
    },
    /// A server of the domain asks to sign in: ask again with
    /// `windows_sign_in` (the signed-in Windows user), or set the account
    /// up by hand (`exchange-autodiscover-needs-login`).
    NeedsLogin {
        /// The host that asked.
        host: String,
        /// The text to show.
        localized: LocalizedMessage,
    },
}

/// What Exchange Autodiscover added to a [`crate::Response::Discovered`].
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
pub struct ExchangeDiscoveryInfo {
    /// Autodiscover stopped to ask (the candidates are what other lookups
    /// found).
    #[serde(default)]
    pub prompt: Option<ExchangeDiscoveryPrompt>,
    /// The user's display name, when a POX answer gave it.
    #[serde(default)]
    pub display_name: Option<String>,
    /// Shared mailboxes the server automaps (stored with origin
    /// `automapped` when the account is added).
    #[serde(default)]
    pub mailboxes: Vec<ExchangeMailboxSettings>,
}

// ---------------------------------------------------------------------------
// Status

/// How the account learns about changes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangePush {
    /// EWS streaming notifications.
    Streaming,
    /// Polling with an adaptive interval.
    Polling,
    /// Not syncing (disabled, offline).
    None,
}

/// What changed for [`crate::Event::ExchangeChanged`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum ExchangeArea {
    /// [`crate::Request::GetExchangeStatus`].
    Status,
    /// [`crate::Request::ListExchangeMailboxes`].
    Mailboxes,
    /// [`crate::Request::ListExchangeCategories`].
    Categories,
    /// [`crate::Request::GetAutomaticReplies`].
    AutomaticReplies,
    /// [`crate::Request::GetExchangeRules`].
    Rules,
    /// [`crate::Request::ListRoomLists`] / [`crate::Request::FindRooms`].
    Rooms,
}

impl ExchangeArea {
    /// Every area.
    pub const ALL: [ExchangeArea; 6] = [
        Self::Status,
        Self::Mailboxes,
        Self::Categories,
        Self::AutomaticReplies,
        Self::Rules,
        Self::Rooms,
    ];
}

/// The Exchange-specific state of an account
/// ([`crate::Request::GetExchangeStatus`]); the connection state itself is
/// [`crate::AccountStatus`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ExchangeStatus {
    /// The account.
    pub account_id: AccountId,
    /// The protocol.
    pub engine: ExchangeEngine,
    /// The server's version as it reports it (EWS
    /// `ServerVersionInfo`), `null` for Graph / unknown.
    #[serde(default)]
    pub server_version: Option<String>,
    /// How changes arrive.
    pub push: ExchangePush,
    /// Last complete sync (RFC 3339 UTC), `null` = never.
    #[serde(default)]
    pub last_sync_at: Option<String>,
    /// The server throttles Nail until then (RFC 3339 UTC).
    #[serde(default)]
    pub throttled_until: Option<String>,
    /// Mail folders synced / known.
    pub folders_synced: u32,
    /// Mail folders known.
    pub folders_total: u32,
    /// Queued Exchange operations (categories, rules, automatic replies,
    /// Focused Inbox) not yet on the server.
    pub pending_ops: u32,
    /// Things the user should know.
    #[serde(default)]
    pub warnings: Vec<ExchangeWarning>,
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn endpoint(host: &str, tls: TlsMode, auth: AuthConfig) -> ServerEndpoint {
        ServerEndpoint {
            host: host.into(),
            port: if tls == TlsMode::Plaintext { 8080 } else { 443 },
            tls,
            auth,
        }
    }

    fn oauth() -> AuthConfig {
        AuthConfig::OAuth2 {
            username: "ada@contoso.example".into(),
            provider: "microsoft".into(),
            secret: None,
            mechanism: None,
        }
    }

    fn windows() -> AuthConfig {
        AuthConfig::Windows {
            username: None,
            secret: None,
            package: None,
        }
    }

    #[test]
    fn engines_and_options() {
        assert_eq!(ExchangeEngine::parse("ews"), Some(ExchangeEngine::Ews));
        assert_eq!(ExchangeEngine::parse("eas"), None);
        assert_eq!(ExchangeOptions::default().engine, ExchangeEngine::Graph);
        assert_eq!(ExchangeOptions::default().poll_range(), (30, 300));
        let ews = ExchangeOptions::for_engine(ExchangeEngine::Ews);
        assert!(ews.streaming() && ews.server_rules());
        assert!(!ExchangeOptions::default().streaming());
        let server = endpoint("mail.contoso.example", TlsMode::Implicit, windows());
        assert_eq!(
            ews.ews_url(&server),
            "https://mail.contoso.example/EWS/Exchange.asmx"
        );
        let local = endpoint("127.0.0.1", TlsMode::Plaintext, oauth());
        assert_eq!(
            ExchangeOptions::default().graph_url(&local),
            "http://127.0.0.1:8080/v1.0"
        );
        let configured = ExchangeOptions {
            url: Some("https://ews.contoso.example/ews/exchange.asmx".into()),
            ..ews.clone()
        };
        assert_eq!(
            configured.ews_url(&server),
            "https://ews.contoso.example/ews/exchange.asmx"
        );
        assert_eq!(serde_json::to_value(&ews).unwrap()["engine"], json!("ews"));
    }

    #[test]
    fn option_issues() {
        let graph = ExchangeOptions::default();
        assert!(
            graph
                .issues(&endpoint(GRAPH_HOST, TlsMode::Implicit, oauth()))
                .is_empty()
        );
        assert_eq!(
            graph.issues(&endpoint(GRAPH_HOST, TlsMode::Implicit, windows())),
            [ExchangeConfigIssue::GraphNeedsOAuth2]
        );
        let ews = ExchangeOptions::for_engine(ExchangeEngine::Ews);
        assert!(
            ews.issues(&endpoint(
                "mail.contoso.example",
                TlsMode::Implicit,
                windows()
            ))
            .is_empty()
        );
        assert_eq!(
            ews.issues(&endpoint(
                "mail.contoso.example",
                TlsMode::Implicit,
                AuthConfig::None
            )),
            [ExchangeConfigIssue::UnsupportedAuth]
        );
        assert_eq!(
            ews.issues(&endpoint(
                "mail.contoso.example",
                TlsMode::Plaintext,
                windows()
            )),
            [ExchangeConfigIssue::PlaintextNeedsLoopback]
        );
        assert!(
            ews.issues(&endpoint("127.0.0.1", TlsMode::Plaintext, windows()))
                .is_empty()
        );
        let mixed = ExchangeOptions {
            streaming: Some(true),
            ..ExchangeOptions::default()
        };
        assert_eq!(
            mixed.issues(&endpoint(GRAPH_HOST, TlsMode::Starttls, oauth())),
            [
                ExchangeConfigIssue::Starttls,
                ExchangeConfigIssue::EwsOnlyOption
            ]
        );
        let bad_url = ExchangeOptions {
            url: Some("ftp://x.example/ews".into()),
            ..ews.clone()
        };
        assert_eq!(
            bad_url.issues(&endpoint("x.example", TlsMode::Implicit, windows())),
            [ExchangeConfigIssue::InvalidUrl]
        );
        for issue in ExchangeConfigIssue::ALL {
            assert!(issue.message_id().starts_with("config-exchange-"));
        }
    }

    #[test]
    fn ews_on_exchange_online_warns() {
        let ews = ExchangeOptions::for_engine(ExchangeEngine::Ews);
        assert_eq!(
            ews.warnings(&endpoint(EXCHANGE_ONLINE_HOST, TlsMode::Implicit, oauth())),
            [ExchangeWarning::EwsRetiredOnline]
        );
        assert!(
            ews.warnings(&endpoint(
                "mail.contoso.example",
                TlsMode::Implicit,
                windows()
            ))
            .is_empty()
        );
        let via_url = ExchangeOptions {
            url: Some("https://outlook.office365.com/EWS/Exchange.asmx".into()),
            ..ews
        };
        assert_eq!(
            via_url.warnings(&endpoint(
                "mail.contoso.example",
                TlsMode::Implicit,
                oauth()
            )),
            [ExchangeWarning::EwsRetiredOnline]
        );
        assert!(is_exchange_online_host("Outlook.Office365.com."));
        assert!(!is_exchange_online_host("notoffice365.com"));
        assert!(is_loopback_host("[::1]") && is_loopback_host("localhost"));
        assert!(!is_loopback_host("10.0.0.1"));
    }

    #[test]
    fn focused_inbox_values() {
        assert_eq!(
            InferenceClassification::parse("Other"),
            Some(InferenceClassification::Other)
        );
        assert_eq!(InferenceClassification::parse("primary"), None);
        assert_eq!(
            serde_json::to_value(InferenceClassification::Focused).unwrap(),
            json!("focused")
        );
        let info: ExchangeMessageInfo =
            serde_json::from_value(json!({"item_id": "AAMk="})).unwrap();
        assert_eq!(info.inference, None);
        assert_eq!(info.mailbox, None);
    }

    #[test]
    fn well_known_folders() {
        assert_eq!(
            ExchangeWellKnownFolder::parse("SentItems"),
            Some(ExchangeWellKnownFolder::SentItems)
        );
        assert_eq!(
            ExchangeWellKnownFolder::parse("archive"),
            Some(ExchangeWellKnownFolder::Archive)
        );
        assert_eq!(
            ExchangeWellKnownFolder::DeletedItems.role(),
            Some(FolderRole::Trash)
        );
        assert_eq!(ExchangeWellKnownFolder::Outbox.role(), None);
        assert_eq!(ExchangeWellKnownFolder::Calendar.graph_name(), None);
        assert_eq!(ExchangeWellKnownFolder::Archive.ews_name(), None);
        for folder in ExchangeWellKnownFolder::ALL {
            let name = folder.graph_name().or(folder.ews_name()).unwrap();
            assert_eq!(ExchangeWellKnownFolder::parse(name), Some(folder));
        }
    }

    #[test]
    fn mailbox_paths_round_trip() {
        assert_eq!(
            mailbox_folder_path(None, "Inbox/Receipts"),
            "Inbox/Receipts"
        );
        assert_eq!(
            mailbox_folder_path(Some("Support@Contoso.example"), "Inbox"),
            "@support@contoso.example/Inbox"
        );
        assert_eq!(
            mailbox_folder_path(Some("support@contoso.example"), ""),
            "@support@contoso.example"
        );
        assert_eq!(
            split_mailbox_folder_path("@support@contoso.example/Inbox/A"),
            (Some("support@contoso.example"), "Inbox/A")
        );
        assert_eq!(
            split_mailbox_folder_path("@support@contoso.example"),
            (Some("support@contoso.example"), "")
        );
        // A user folder starting with @ but without an address stays the
        // account's own.
        assert_eq!(split_mailbox_folder_path("@Later"), (None, "@Later"));
        let source = exchange_pim_source_id(&"work".into());
        assert_eq!(source.as_str(), "exchange:work");
        assert_eq!(exchange_pim_source_account(&source), Some("work".into()));
        assert_eq!(exchange_pim_source_account(&PimSourceId::local()), None);
        assert!(!crate::pim::source::is_valid_pim_source_id(source.as_str()));
    }

    #[test]
    fn mailbox_settings_issues() {
        let own = "ada@contoso.example";
        assert!(
            ExchangeMailboxSettings::shared("support@contoso.example")
                .issues(own)
                .is_empty()
        );
        assert_eq!(
            ExchangeMailboxSettings::shared(" ").issues(own),
            [ExchangeMailboxIssue::MissingAddress]
        );
        assert_eq!(
            ExchangeMailboxSettings::shared("support").issues(own),
            [ExchangeMailboxIssue::InvalidAddress]
        );
        assert_eq!(
            ExchangeMailboxSettings::shared("ADA@contoso.example").issues(own),
            [ExchangeMailboxIssue::OwnMailbox]
        );
        let mut nothing = ExchangeMailboxSettings::shared("boss@contoso.example");
        nothing.kind = ExchangeMailboxKind::Delegate;
        nothing.mail = false;
        nothing.calendar = false;
        nothing.name = Some(String::new());
        assert_eq!(
            nothing.issues(own),
            [
                ExchangeMailboxIssue::EmptyName,
                ExchangeMailboxIssue::NothingShown
            ]
        );
        let parsed: ExchangeMailboxSettings =
            serde_json::from_value(json!({"address": "x@y.example", "kind": "shared"})).unwrap();
        assert!(parsed.mail && parsed.calendar && !parsed.contacts && !parsed.tasks);
        for state in ExchangeMailboxState::ALL {
            assert_eq!(ExchangeMailboxState::parse(state.as_str()), Some(state));
            assert_eq!(serde_json::to_value(state).unwrap(), json!(state.as_str()));
        }
        for issue in ExchangeMailboxIssue::ALL {
            assert!(issue.message_id().starts_with("exchange-mailbox-issue-"));
        }
    }

    #[test]
    fn category_colors() {
        assert_eq!(CategoryColor::ALL.len(), 26);
        assert_eq!(CategoryColor::Preset7.as_str(), "preset7");
        assert_eq!(
            CategoryColor::parse("Preset24"),
            Some(CategoryColor::Preset24)
        );
        assert_eq!(CategoryColor::None.ews_index(), -1);
        assert_eq!(CategoryColor::Preset0.ews_index(), 0);
        assert_eq!(CategoryColor::Preset24.ews_index(), 24);
        assert_eq!(CategoryColor::from_ews_index(3), CategoryColor::Preset3);
        assert_eq!(CategoryColor::from_ews_index(99), CategoryColor::None);
        assert_eq!(CategoryColor::None.rgb(), None);
        assert_eq!(CategoryColor::Preset0.rgb(), Some("#e7a1a2"));
        for color in CategoryColor::ALL {
            assert_eq!(serde_json::to_value(color).unwrap(), json!(color.as_str()));
            assert_eq!(CategoryColor::from_ews_index(color.ews_index()), color);
            if let Some(rgb) = color.rgb() {
                assert!(rgb.len() == 7 && rgb.starts_with('#'));
            }
        }
    }

    #[test]
    fn category_settings_issues() {
        assert!(
            ExchangeCategorySettings::named("Project X")
                .issues()
                .is_empty()
        );
        assert_eq!(
            ExchangeCategorySettings::named("").issues(),
            [ExchangeCategoryIssue::MissingName]
        );
        assert_eq!(
            ExchangeCategorySettings::named("a,b").issues(),
            [ExchangeCategoryIssue::InvalidName]
        );
        assert_eq!(
            ExchangeCategorySettings::named("x".repeat(256)).issues(),
            [ExchangeCategoryIssue::NameTooLong]
        );
        for issue in ExchangeCategoryIssue::ALL {
            assert!(issue.message_id().starts_with("exchange-category-issue-"));
        }
        let category: ExchangeCategory = serde_json::from_value(json!({
            "account_id": "work", "id": "c1", "name": "Red", "color": "preset0",
            "total": 2, "unread": 1
        }))
        .unwrap();
        assert_eq!(category.sync, ExchangeSync::Synced);
        assert_eq!(category.settings().color, CategoryColor::Preset0);
        assert_eq!(ExchangeSync::parse("pending"), ExchangeSync::Pending);
    }

    #[test]
    fn collection_roles_and_warnings() {
        for role in ExchangeCollectionRole::ALL {
            assert_eq!(ExchangeCollectionRole::parse(role.as_str()), Some(role));
            assert_eq!(serde_json::to_value(role).unwrap(), json!(role.as_str()));
        }
        for warning in ExchangeWarning::ALL {
            assert!(warning.message_id().starts_with("exchange-warning-"));
        }
        assert_eq!(
            serde_json::to_value(ExchangeArea::AutomaticReplies).unwrap(),
            json!("automatic-replies")
        );
    }
}
