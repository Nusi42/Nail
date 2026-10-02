//! Account configuration: real accounts, identities, virtual accounts and
//! OAuth2 providers, as stored in `accounts.kdl` (parsed by `nail-config`)
//! and exchanged with the UI (account setup, settings).
//!
//! Secrets never appear in these types: passwords, OAuth2 refresh tokens and
//! client certificates live in the OS keychain and are referenced by a
//! [`SecretRef`]. When the UI hands a new secret to the core (account setup),
//! it travels once as a [`SecretInput`] next to the configuration, and the
//! core stores it in the keychain.
//!
//! Wire and file conventions: `docs/adr/0013-accounts-file.md`.

use std::fmt;

use serde::{Deserialize, Serialize};
use ts_rs::TS;

use crate::domain::FolderRole;
use crate::error::ErrorCode;
use crate::ids::{AccountId, IdentityId, OAuthFlowId};
use crate::localized::LocalizedMessage;
use crate::security::IdentityCrypto;
use crate::sieve::ManageSieveConfig;

/// The whole `accounts.kdl` file: every configured account, virtual account
/// and OAuth2 provider, in file order (which is the display order).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct AccountsConfig {
    /// Real accounts.
    pub accounts: Vec<AccountConfig>,
    /// User-defined virtual accounts. The built-in "All accounts" virtual
    /// account ([`AccountId::ALL`]) is not listed here.
    pub virtual_accounts: Vec<VirtualAccountConfig>,
    /// OAuth2 client registrations (`oauth-provider "google" client-id=…`).
    pub oauth_providers: Vec<OAuthProviderConfig>,
    /// Undo-send delay in seconds applied to [`crate::Request::SendDraft`]
    /// when the request does not give one. `null` means
    /// [`AccountsConfig::DEFAULT_UNDO_SEND_SECS`].
    pub undo_send_secs: Option<u32>,
    /// Calendar, contact and task sources: ICS subscriptions, CalDAV and
    /// CardDAV servers (`calendar-subscription`, `caldav`, `carddav` nodes;
    /// M3a, ADR 0058), in file order. The built-in local source is not
    /// listed.
    pub pim_sources: Vec<crate::pim::PimSourceConfig>,
}

impl AccountsConfig {
    /// Undo-send delay used when neither the file nor the request sets one.
    pub const DEFAULT_UNDO_SEND_SECS: u32 = 10;

    /// The account with this id.
    pub fn account(&self, id: &AccountId) -> Option<&AccountConfig> {
        self.accounts.iter().find(|a| &a.id == id)
    }

    /// The virtual account with this id.
    pub fn virtual_account(&self, id: &AccountId) -> Option<&VirtualAccountConfig> {
        self.virtual_accounts.iter().find(|v| &v.id == id)
    }

    /// The PIM source with this id.
    pub fn pim_source(&self, id: &crate::ids::PimSourceId) -> Option<&crate::pim::PimSourceConfig> {
        self.pim_sources.iter().find(|s| &s.id == id)
    }

    /// The identity with this id, in any account.
    pub fn identity(&self, id: &IdentityId) -> Option<&Identity> {
        self.accounts
            .iter()
            .flat_map(|a| a.identities.iter())
            .find(|i| &i.id == id)
    }

    /// The effective undo-send delay in seconds.
    pub fn effective_undo_send_secs(&self) -> u32 {
        self.undo_send_secs.unwrap_or(Self::DEFAULT_UNDO_SEND_SECS)
    }
}

/// One real mail account: incoming server, optional outgoing server,
/// identities and sync policy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountConfig {
    /// Stable id, the name in the file (`account "work"`). Lowercase
    /// letters, digits, `-`, `_` and `.`, starting with a letter or digit, at
    /// most 64 characters; unique among accounts and virtual accounts; `all`
    /// is reserved. In [`crate::Request::AddAccount`] an empty id asks the
    /// core to derive one from the email address.
    pub id: AccountId,
    /// Display name, e.g. "Work".
    pub name: String,
    /// Primary email address.
    pub email: String,
    /// Whether the account is synchronized. Disabled accounts stay in the
    /// file and keep their local mail.
    #[serde(default = "default_true")]
    pub enabled: bool,
    /// Where mail comes from.
    pub incoming: IncomingServer,
    /// Where mail is sent through; `null` for receive-only accounts.
    #[serde(default)]
    pub outgoing: Option<OutgoingServer>,
    /// Sending identities; the first one is the account's default. Never
    /// empty after parsing: a file without identities gets one built from
    /// the account's `email` (id = the address, no display name).
    #[serde(default)]
    pub identities: Vec<Identity>,
    /// Which folders are synchronized and how much of each message.
    #[serde(default)]
    pub sync: SyncPolicy,
    /// Whether a copy of each sent message is appended to the Sent folder.
    /// `null` = decide automatically (skipped for servers that store sent
    /// mail themselves, such as Gmail).
    #[serde(default)]
    pub save_sent_copy: Option<bool>,
    /// `authserv-id` values of the account's own receiving servers (M2a):
    /// only `Authentication-Results` headers from these produce the sender
    /// check badge (RFC 8601 §5). Empty = no badge; ids are never
    /// trusted without being configured here
    /// ([`crate::Request::SuggestAuthservId`] proposes one).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    #[ts(as = "Option<Vec<String>>", optional)]
    pub authserv_ids: Vec<String>,
    /// ManageSieve (M2c): server-side rules and vacation replies, IMAP
    /// accounts only (the `sieve` node). Absent = off; rules run on the
    /// client.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub sieve: Option<ManageSieveConfig>,
}

fn default_true() -> bool {
    true
}

impl AccountConfig {
    /// The protocol family of the account.
    pub fn kind(&self) -> crate::domain::AccountKind {
        match self.incoming {
            IncomingServer::Imap { .. } => crate::domain::AccountKind::Imap,
            IncomingServer::Pop3 { .. } => crate::domain::AccountKind::Pop3,
            IncomingServer::Gmail { .. } => crate::domain::AccountKind::Gmail,
            IncomingServer::Exchange { .. } => crate::domain::AccountKind::Exchange,
        }
    }

    /// Whether the account can send: an outgoing server, or an API account
    /// that sends through its API (Gmail `messages.send`, Exchange).
    pub fn can_send(&self) -> bool {
        self.outgoing.is_some() || self.incoming.sends_itself()
    }

    /// The default identity (the first one).
    pub fn default_identity(&self) -> Option<&Identity> {
        self.identities.first()
    }
}

/// The incoming side of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum IncomingServer {
    /// IMAP: the local store mirrors the server.
    Imap {
        /// Server, security and login.
        server: ServerEndpoint,
        /// IMAP-specific settings.
        #[serde(default)]
        options: ImapOptions,
    },
    /// POP3: mail is downloaded into local folders.
    Pop3 {
        /// Server, security and login.
        server: ServerEndpoint,
        /// POP3-specific settings.
        #[serde(default)]
        options: Pop3Options,
    },
    /// The Gmail API (M3b, ADR 0065): sync through `users.history`,
    /// sending through `users.messages.send` (the account has no
    /// `outgoing` server). `server` is the API host (default
    /// [`crate::gmail::GMAIL_API_HOST`], port 443, implicit TLS; plain
    /// `http` only for loopback test servers) with `oauth2` auth.
    Gmail {
        /// API host, security and login.
        server: ServerEndpoint,
        /// Gmail-specific settings.
        #[serde(default)]
        options: crate::gmail::GmailOptions,
    },
    /// Microsoft Exchange (M3c, ADR 0072): Graph for Exchange Online, EWS
    /// for Exchange Server on-premises
    /// ([`crate::exchange::ExchangeOptions::engine`]); mail, calendars,
    /// contacts and tasks over one protocol, sending through it too (no
    /// `outgoing` server). `server` is the Graph host
    /// ([`crate::exchange::GRAPH_HOST`]) or the Exchange server (EWS),
    /// port 443, implicit TLS (plain `http` only for loopback test
    /// servers); auth `oauth2` (Graph, EWS hybrid), `windows` (EWS:
    /// Negotiate / Kerberos / NTLM) or `password` (EWS: Basic over TLS).
    Exchange {
        /// Host, security and login.
        server: ServerEndpoint,
        /// Exchange-specific settings.
        #[serde(default)]
        options: crate::exchange::ExchangeOptions,
    },
}

impl IncomingServer {
    /// The server endpoint, whatever the protocol.
    pub fn server(&self) -> &ServerEndpoint {
        match self {
            Self::Imap { server, .. }
            | Self::Pop3 { server, .. }
            | Self::Gmail { server, .. }
            | Self::Exchange { server, .. } => server,
        }
    }

    /// The protocol of this side.
    pub fn protocol(&self) -> MailProtocol {
        match self {
            Self::Imap { .. } => MailProtocol::Imap,
            Self::Pop3 { .. } => MailProtocol::Pop3,
            Self::Gmail { .. } => MailProtocol::Gmail,
            Self::Exchange { options, .. } => match options.engine {
                crate::exchange::ExchangeEngine::Graph => MailProtocol::Graph,
                crate::exchange::ExchangeEngine::Ews => MailProtocol::Ews,
            },
        }
    }

    /// Whether this side also sends (API accounts): then the account has
    /// no `outgoing` server.
    pub fn sends_itself(&self) -> bool {
        matches!(self, Self::Gmail { .. } | Self::Exchange { .. })
    }
}

/// The outgoing side of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "protocol", rename_all = "lowercase")]
pub enum OutgoingServer {
    /// SMTP submission (RFC 6409).
    Smtp {
        /// Server, security and login.
        server: ServerEndpoint,
        /// SMTP-specific settings.
        #[serde(default)]
        options: SmtpOptions,
    },
}

impl OutgoingServer {
    /// The server endpoint, whatever the protocol.
    pub fn server(&self) -> &ServerEndpoint {
        match self {
            Self::Smtp { server, .. } => server,
        }
    }
}

/// How to reach and log in to one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ServerEndpoint {
    /// Host name or IP address; also the TLS server name.
    pub host: String,
    /// TCP port. The file may omit it; the reader then fills in the
    /// protocol's default for the TLS mode (see [`default_port`]).
    pub port: u16,
    /// Transport security.
    pub tls: TlsMode,
    /// How to authenticate.
    pub auth: AuthConfig,
}

/// Transport security of a connection.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum TlsMode {
    /// TLS from the first byte (IMAP 993, POP3 995, SMTP 465; RFC 8314).
    #[default]
    Implicit,
    /// Plaintext connect, then STARTTLS / STLS before authenticating (IMAP
    /// 143, POP3 110, SMTP 587). Never falls back to plaintext.
    Starttls,
    /// No TLS at all. Credentials travel in clear text: only for local test
    /// servers.
    Plaintext,
}

/// A mail protocol, for port defaults and discovery results.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum MailProtocol {
    /// IMAP.
    Imap,
    /// POP3.
    Pop3,
    /// SMTP submission.
    Smtp,
    /// The Gmail REST API (HTTPS; M3b).
    Gmail,
    /// Microsoft Graph (HTTPS; M3c, Exchange Online).
    Graph,
    /// Exchange Web Services (HTTPS; M3c, Exchange on-premises).
    Ews,
}

/// The standard port of `protocol` with `tls`.
pub fn default_port(protocol: MailProtocol, tls: TlsMode) -> u16 {
    match (protocol, tls) {
        (MailProtocol::Imap, TlsMode::Implicit) => 993,
        (MailProtocol::Imap, _) => 143,
        (MailProtocol::Pop3, TlsMode::Implicit) => 995,
        (MailProtocol::Pop3, _) => 110,
        (MailProtocol::Smtp, TlsMode::Implicit) => 465,
        (MailProtocol::Smtp, _) => 587,
        (MailProtocol::Gmail | MailProtocol::Graph | MailProtocol::Ews, TlsMode::Implicit) => 443,
        (MailProtocol::Gmail | MailProtocol::Graph | MailProtocol::Ews, _) => 80,
    }
}

/// How to authenticate to a server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "method")]
pub enum AuthConfig {
    /// User name and password (or app password) from the keychain.
    #[serde(rename = "password")]
    Password {
        /// Login name, often the email address.
        username: String,
        /// Where the password is stored. `null` = the account's default
        /// reference (see [`SecretRef::default_for`] with
        /// [`SecretKind::Password`]).
        secret: Option<SecretRef>,
        /// SASL mechanism; `null` = choose the best one the server offers.
        mechanism: Option<SaslMechanism>,
    },
    /// OAuth2 (XOAUTH2 / OAUTHBEARER) with a refresh token from the keychain.
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// Login name (the email address for Google and Microsoft).
        username: String,
        /// Provider id: `google`, `microsoft` or an `oauth-provider` defined
        /// in the file.
        provider: String,
        /// Where the refresh token is stored. `null` = the account's default
        /// reference ([`SecretKind::OAuth2`]).
        secret: Option<SecretRef>,
        /// SASL mechanism, `xoauth2` or `oauthbearer`; `null` = choose.
        mechanism: Option<SaslMechanism>,
    },
    /// TLS client certificate (SASL EXTERNAL where required).
    #[serde(rename = "client-certificate")]
    ClientCertificate {
        /// Authorization identity, if the server needs one.
        username: Option<String>,
        /// Where the PEM certificate chain and private key are stored.
        /// `null` = the account's default reference
        /// ([`SecretKind::ClientCertificate`]).
        secret: Option<SecretRef>,
    },
    /// Windows integrated sign-in (M3c, Exchange EWS only): HTTP
    /// Negotiate (Kerberos, falling back to NTLM) through SSPI. Without a
    /// `username` Nail signs in as the signed-in Windows user (no secret
    /// stored); with one, the password comes from the keychain.
    #[serde(rename = "windows")]
    Windows {
        /// `DOMAIN\user` or `user@domain`; `null` = the signed-in Windows
        /// user.
        username: Option<String>,
        /// Where the password of `username` is stored. `null` = the
        /// account's default reference ([`SecretKind::Password`]).
        secret: Option<SecretRef>,
        /// The SSPI package; `null` = Negotiate.
        package: Option<SspiPackage>,
    },
    /// No authentication (e.g. an SMTP relay on the local network).
    #[serde(rename = "none")]
    None,
}

/// The SSPI security package of [`AuthConfig::Windows`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum SspiPackage {
    /// SPNEGO: Kerberos, else NTLM (the default).
    #[default]
    Negotiate,
    /// Kerberos only.
    Kerberos,
    /// NTLM only.
    Ntlm,
}

impl SspiPackage {
    /// The SSPI package name (`Negotiate`, `Kerberos`, `NTLM`).
    pub fn sspi_name(self) -> &'static str {
        match self {
            Self::Negotiate => "Negotiate",
            Self::Kerberos => "Kerberos",
            Self::Ntlm => "NTLM",
        }
    }

    /// The name in `accounts.kdl` (`negotiate`, `kerberos`, `ntlm`).
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Negotiate => "negotiate",
            Self::Kerberos => "kerberos",
            Self::Ntlm => "ntlm",
        }
    }

    /// The package of a name.
    pub fn parse(name: &str) -> Option<Self> {
        [Self::Negotiate, Self::Kerberos, Self::Ntlm]
            .into_iter()
            .find(|p| p.as_str() == name)
    }
}

impl AuthConfig {
    /// The kind of secret this method needs, `None` for [`AuthConfig::None`].
    pub fn secret_kind(&self) -> Option<SecretKind> {
        match self {
            Self::Password { .. } => Some(SecretKind::Password),
            Self::OAuth2 { .. } => Some(SecretKind::OAuth2),
            Self::ClientCertificate { .. } => Some(SecretKind::ClientCertificate),
            // The signed-in Windows user needs no stored secret.
            Self::Windows { username, .. } => username.as_ref().map(|_| SecretKind::Password),
            Self::None => None,
        }
    }

    /// The explicitly configured secret reference, if any.
    pub fn explicit_secret(&self) -> Option<&SecretRef> {
        match self {
            Self::Password { secret, .. }
            | Self::OAuth2 { secret, .. }
            | Self::ClientCertificate { secret, .. }
            | Self::Windows { secret, .. } => secret.as_ref(),
            Self::None => None,
        }
    }

    /// The secret reference to use for `account`: the explicit one, else the
    /// account's default for this method. `None` for [`AuthConfig::None`].
    pub fn secret_ref(&self, account: &AccountId) -> Option<SecretRef> {
        let kind = self.secret_kind()?;
        Some(
            self.explicit_secret()
                .cloned()
                .unwrap_or_else(|| SecretRef::default_for(account, kind)),
        )
    }
}

/// A SASL mechanism (or the IMAP `LOGIN` / POP3 `USER`/`PASS`/`APOP` commands).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SaslMechanism {
    /// `PLAIN` (RFC 4616).
    Plain,
    /// `LOGIN` (SASL LOGIN, IMAP `LOGIN`, POP3 `USER`/`PASS`).
    Login,
    /// `SCRAM-SHA-256` (RFC 7677).
    ScramSha256,
    /// POP3 `APOP`.
    Apop,
    /// `XOAUTH2` (Google, Microsoft).
    Xoauth2,
    /// `OAUTHBEARER` (RFC 7628).
    Oauthbearer,
    /// `EXTERNAL` (client certificate).
    External,
}

/// Which kind of secret a [`SecretRef`] points to.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SecretKind {
    /// A password.
    Password,
    /// An OAuth2 refresh token (plus cached access token).
    #[serde(rename = "oauth2")]
    OAuth2,
    /// A PEM client certificate chain with its private key.
    ClientCertificate,
}

impl SecretKind {
    /// The last segment of the default reference, e.g. `password`.
    pub fn slot(self) -> &'static str {
        match self {
            Self::Password => "password",
            Self::OAuth2 => "oauth2",
            Self::ClientCertificate => "client-certificate",
        }
    }
}

/// A reference to a secret in the OS keychain, as written in `accounts.kdl`:
/// `keychain:<service>/<name>`, e.g. `keychain:nail/work/password`.
///
/// The reference is the only thing about a secret that is ever stored in a
/// file or sent to a client. The keychain entry behind it is addressed as
/// service `<service>` and user `<name>` (see
/// [`crate::engine::SecretStore`]).
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize, TS)]
pub struct SecretRef(pub String);

impl SecretRef {
    /// The scheme every reference starts with.
    pub const SCHEME: &'static str = "keychain:";
    /// The keychain service Nail's own entries use.
    pub const SERVICE: &'static str = "nail";

    /// Parses and validates a reference: `keychain:` followed by a service,
    /// `/` and a non-empty name, without whitespace or control characters.
    pub fn parse(text: &str) -> Result<Self, SecretRefError> {
        let rest = text
            .strip_prefix(Self::SCHEME)
            .ok_or(SecretRefError::Scheme)?;
        let (service, name) = rest.split_once('/').ok_or(SecretRefError::Shape)?;
        if service.is_empty() || name.is_empty() || name.ends_with('/') {
            return Err(SecretRefError::Shape);
        }
        if rest.chars().any(|c| c.is_whitespace() || c.is_control()) {
            return Err(SecretRefError::Characters);
        }
        if text.len() > 512 {
            return Err(SecretRefError::Length);
        }
        Ok(Self(text.to_owned()))
    }

    /// The default reference for a secret of `account`:
    /// `keychain:nail/<account>/<kind slot>`.
    pub fn default_for(account: &AccountId, kind: SecretKind) -> Self {
        Self(format!(
            "{}{}/{}/{}",
            Self::SCHEME,
            Self::SERVICE,
            account,
            kind.slot()
        ))
    }

    /// The reference as written, e.g. `keychain:nail/work/password`.
    pub fn as_str(&self) -> &str {
        &self.0
    }

    /// The keychain service (`nail` in `keychain:nail/work/password`).
    /// Empty for an unvalidated, malformed reference.
    pub fn service(&self) -> &str {
        self.parts().0
    }

    /// The keychain entry name (`work/password` in
    /// `keychain:nail/work/password`).
    pub fn name(&self) -> &str {
        self.parts().1
    }

    fn parts(&self) -> (&str, &str) {
        self.0
            .strip_prefix(Self::SCHEME)
            .and_then(|rest| rest.split_once('/'))
            .unwrap_or(("", ""))
    }
}

impl fmt::Display for SecretRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

/// Why a string is not a valid [`SecretRef`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum SecretRefError {
    /// It does not start with `keychain:`.
    #[error(
        "secrets are stored in the OS keychain; write a reference like \"keychain:nail/<account>/password\""
    )]
    Scheme,
    /// It lacks the `<service>/<name>` part.
    #[error("a keychain reference has the form \"keychain:<service>/<name>\"")]
    Shape,
    /// It contains whitespace or control characters.
    #[error("a keychain reference must not contain spaces or control characters")]
    Characters,
    /// It is longer than 512 bytes.
    #[error("a keychain reference must be at most 512 bytes long")]
    Length,
}

/// IMAP-specific account settings. Every field is optional; `null` means
/// the documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct ImapOptions {
    /// Seconds between polls of folders that are not watched with IDLE, and
    /// of all folders when the server has no IDLE. Default 300.
    pub poll_interval_secs: Option<u32>,
    /// Use `COMPRESS=DEFLATE` when offered. Default true.
    pub compress: Option<bool>,
    /// At most this many simultaneous connections. Default 4.
    pub max_connections: Option<u32>,
}

impl ImapOptions {
    /// Default of [`Self::poll_interval_secs`].
    pub const DEFAULT_POLL_INTERVAL_SECS: u32 = 300;
    /// Default of [`Self::max_connections`].
    pub const DEFAULT_MAX_CONNECTIONS: u32 = 4;
}

/// POP3-specific account settings. `null` means the documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Pop3Options {
    /// Seconds between downloads. Default 600.
    pub poll_interval_secs: Option<u32>,
    /// Keep messages on the server after downloading them. Default true.
    pub leave_on_server: Option<bool>,
    /// With `leave_on_server`: delete a message from the server this many
    /// days after it was downloaded. `null` = never.
    pub delete_after_days: Option<u32>,
    /// With `leave_on_server`: delete a message from the server when it is
    /// deleted locally (emptied from Trash). Default false.
    pub delete_when_deleted_locally: Option<bool>,
}

impl Pop3Options {
    /// Default of [`Self::poll_interval_secs`].
    pub const DEFAULT_POLL_INTERVAL_SECS: u32 = 600;
}

/// SMTP-specific account settings. `null` means the documented default.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct SmtpOptions {
    /// Request delivery status notifications (DSN, RFC 3461) for failures.
    /// Default false.
    pub dsn: Option<bool>,
}

/// Which folders are synchronized and how.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct SyncPolicy {
    /// What to download for folders without a rule. `null` = full bodies.
    pub bodies: Option<BodyPolicy>,
    /// Per-folder rules. When several match a folder, later rules override
    /// earlier ones field by field.
    pub folders: Vec<FolderPolicy>,
}

impl SyncPolicy {
    /// The effective policy for a folder with `path` and `role`: defaults
    /// (full bodies, synced, IDLE only on the inbox), then every matching
    /// rule in order.
    pub fn effective(&self, path: &str, role: FolderRole) -> EffectiveFolderPolicy {
        let mut out = EffectiveFolderPolicy {
            bodies: self.bodies.unwrap_or(BodyPolicy::Full),
            enabled: true,
            idle: role == FolderRole::Inbox,
        };
        for rule in self.folders.iter().filter(|r| r.folder.matches(path, role)) {
            if let Some(bodies) = rule.bodies {
                out.bodies = bodies;
            }
            if let Some(enabled) = rule.enabled {
                out.enabled = enabled;
            }
            if let Some(idle) = rule.idle {
                out.idle = idle;
            }
        }
        out
    }
}

/// The resolved sync settings of one folder ([`SyncPolicy::effective`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EffectiveFolderPolicy {
    /// What to download.
    pub bodies: BodyPolicy,
    /// Whether the folder is synchronized at all.
    pub enabled: bool,
    /// Whether the folder is watched with IMAP IDLE.
    pub idle: bool,
}

/// How much of each message is downloaded.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(tag = "mode", rename_all = "kebab-case")]
pub enum BodyPolicy {
    /// Headers and full bodies of every message.
    #[default]
    Full,
    /// Headers only; bodies are fetched when a message is opened.
    HeadersOnly,
    /// Full bodies of messages from the last `days` days, headers only for
    /// older ones.
    Recent {
        /// Age limit in days (at least 1).
        days: u32,
    },
}

/// Sync settings for the folders matched by [`FolderPolicy::folder`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FolderPolicy {
    /// Which folder(s) the rule applies to.
    pub folder: FolderMatch,
    /// What to download; `null` = unchanged.
    #[serde(default)]
    pub bodies: Option<BodyPolicy>,
    /// Whether the folder is synchronized; `null` = unchanged (default true).
    #[serde(default)]
    pub enabled: Option<bool>,
    /// Whether the folder is watched with IDLE; `null` = unchanged (default:
    /// only the inbox).
    #[serde(default)]
    pub idle: Option<bool>,
}

/// Selects folders of an account, by server path or by special-use role.
///
/// JSON: `{"path": "Lists/rust"}` or `{"role": "archive"}`.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum FolderMatch {
    /// The folder with this full server path (case-sensitive; `INBOX` is
    /// matched case-insensitively as IMAP requires).
    Path(String),
    /// The folder with this role.
    Role(FolderRole),
}

impl FolderMatch {
    /// Whether a folder with `path` and `role` is selected.
    pub fn matches(&self, path: &str, role: FolderRole) -> bool {
        match self {
            Self::Path(p) => {
                p == path || (p.eq_ignore_ascii_case("INBOX") && path.eq_ignore_ascii_case("INBOX"))
            }
            Self::Role(r) => *r == role && role != FolderRole::Other,
        }
    }
}

/// A sending identity of an account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Identity {
    /// Stable id (see [`IdentityId`]).
    pub id: IdentityId,
    /// The account the identity sends through.
    pub account_id: AccountId,
    /// Sender address.
    pub email: String,
    /// Sender display name, e.g. "Ada Lovelace"; `null` = address only.
    #[serde(default)]
    pub name: Option<String>,
    /// `Reply-To` address, if different from `email`.
    #[serde(default)]
    pub reply_to: Option<String>,
    /// Addresses always put in `Bcc`.
    #[serde(default)]
    pub bcc: Vec<String>,
    /// Signature appended by the composer; `null` = none.
    #[serde(default)]
    pub signature: Option<Signature>,
    /// Composer mode new messages from this identity start in; `null` =
    /// plain text (M1a) / the app default.
    #[serde(default)]
    pub composer_mode: Option<ComposerMode>,
    /// Where replies put the quoted original relative to the new text;
    /// `null` = below it (bottom posting).
    #[serde(default)]
    pub posting: Option<QuotePosting>,
    /// Stylesheet and code-highlighting theme of HTML / Markdown mail from
    /// this identity; `null` = the built-in defaults.
    #[serde(default)]
    pub mail_style: Option<MailStyleConfig>,
    /// End-to-end settings (M2a: default sign / encrypt, preferred
    /// technology, own key, Autocrypt); absent = all defaults
    /// ([`IdentityCrypto::default`]). Saved with
    /// [`crate::Request::UpdateAccount`] like every identity setting.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub crypto: Option<IdentityCrypto>,
    /// Sending as / on behalf of another mailbox (a shared or delegate
    /// Exchange mailbox; M4c, ADR 0094): `email` is then that mailbox's
    /// address. Absent = the account's own address.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub send_as: Option<SendAs>,
}

/// How an identity sends for another mailbox (`send-as=` of an identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum SendAsMode {
    /// "Send as": the mail is from the mailbox alone (needs the Send As
    /// permission on the server).
    SendAs,
    /// "Send on behalf": `From` is the mailbox, `Sender` the user (needs
    /// the Send on Behalf permission); recipients see "Ada on behalf of
    /// Team".
    OnBehalf,
}

impl SendAsMode {
    /// The `accounts.kdl` / wire name.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::SendAs => "send-as",
            Self::OnBehalf => "on-behalf",
        }
    }
}

/// Sending for another mailbox.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SendAs {
    /// Send as or on behalf.
    pub mode: SendAsMode,
    /// The user's own address put into `Sender` for `on-behalf`; `null` =
    /// the account's own address (the core fills it in when sending).
    #[serde(default)]
    pub sender: Option<String>,
}

/// Which mailbox an API engine (Graph, EWS, Gmail) sends a finished
/// message from, read from its `From` / `Sender` headers and the account's
/// own address (M4c): the account itself, or another mailbox as / on
/// behalf.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SendingMailbox {
    /// The account's own mailbox.
    Own,
    /// Another mailbox, "send as".
    SendAs {
        /// The mailbox's address.
        mailbox: String,
    },
    /// Another mailbox, on behalf of the user.
    OnBehalf {
        /// The mailbox's address.
        mailbox: String,
    },
}

impl SendingMailbox {
    /// Decides from the message's `From` address, its `Sender` address (if
    /// any) and the account's own address (case-insensitive).
    pub fn of(from: &str, sender: Option<&str>, own: &str) -> Self {
        let same = |a: &str, b: &str| a.trim().eq_ignore_ascii_case(b.trim());
        if same(from, own) {
            return Self::Own;
        }
        match sender {
            Some(sender) if !sender.trim().is_empty() && !same(sender, from) => Self::OnBehalf {
                mailbox: from.trim().to_owned(),
            },
            _ => Self::SendAs {
                mailbox: from.trim().to_owned(),
            },
        }
    }
}

/// Top or bottom posting in replies (`posting=` of an identity).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum QuotePosting {
    /// New text above the quote.
    Top,
    /// New text below the quote (the default).
    #[default]
    Bottom,
}

/// An identity's mail style (`mail-style` in `accounts.kdl`).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct MailStyleConfig {
    /// CSS inlined into outgoing HTML; `null` = the built-in stylesheet.
    pub css: Option<String>,
    /// Code-highlighting theme for Markdown code blocks (a syntect theme
    /// name, e.g. `InspiredGitHub`); `null` = the default.
    pub code_theme: Option<String>,
}

/// A signature. M1a uses only `text`; the HTML and Markdown variants are
/// reserved for the rich composer (M1b).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize, TS)]
#[serde(default)]
pub struct Signature {
    /// Plain-text signature (without the `-- ` separator line).
    pub text: Option<String>,
    /// HTML variant (M1b).
    pub html: Option<String>,
    /// Markdown variant (M1b).
    pub markdown: Option<String>,
}

/// The composer's editing mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum ComposerMode {
    /// Plain text (`text/plain; format=flowed`).
    #[default]
    Plain,
    /// HTML (M1b).
    Html,
    /// Markdown (M1b).
    Markdown,
}

/// A user-defined virtual account: several real accounts shown as one.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct VirtualAccountConfig {
    /// Stable id, the name in the file (`virtual-account "everything"`);
    /// same rules as [`AccountConfig::id`]. In
    /// [`crate::Request::AddVirtualAccount`] an empty id asks the core to
    /// derive one from the name.
    pub id: AccountId,
    /// Display name.
    pub name: String,
    /// Member accounts (real accounts only), in display order.
    pub members: Vec<AccountId>,
    /// Merge the members' folders with the same special-use role (Inbox,
    /// Sent, Drafts, Archive, Junk, Trash). Default true.
    #[serde(default = "default_true")]
    pub merge_roles: bool,
    /// Hand-made folder merges, e.g. "Receipts" = work/Receipts +
    /// personal/Bills. Unmapped folders appear under a per-account subtree.
    #[serde(default)]
    pub folder_mappings: Vec<FolderMapping>,
    /// Identity for new messages; `null` = the first member's default
    /// identity.
    #[serde(default)]
    pub default_identity: Option<IdentityId>,
    /// Identity overrides by recipient domain, first match wins.
    #[serde(default)]
    pub identity_rules: Vec<IdentityRule>,
}

/// One merged folder of a virtual account.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct FolderMapping {
    /// Folder name shown in the virtual account.
    pub name: String,
    /// The member folders merged into it (at least one).
    pub sources: Vec<FolderSource>,
}

/// A folder of a member account.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
pub struct FolderSource {
    /// The member account.
    pub account_id: AccountId,
    /// Full server path of the folder in that account.
    pub path: String,
}

/// Picks an identity by the recipients' domain (`*@client.com` → work).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct IdentityRule {
    /// Address pattern: `*@example.com` (a domain), `*@*.example.com`
    /// (subdomains) or a full address. Case-insensitive.
    pub pattern: String,
    /// The identity to use when any recipient matches.
    pub identity: IdentityId,
}

impl IdentityRule {
    /// Whether `address` matches [`Self::pattern`].
    pub fn matches(&self, address: &str) -> bool {
        let address = address.trim().to_ascii_lowercase();
        let pattern = self.pattern.trim().to_ascii_lowercase();
        if let Some(domain) = pattern.strip_prefix("*@*.") {
            address
                .rsplit_once('@')
                .is_some_and(|(_, d)| d.ends_with(&format!(".{domain}")))
        } else if let Some(domain) = pattern.strip_prefix("*@") {
            address.rsplit_once('@').is_some_and(|(_, d)| d == domain)
        } else {
            address == pattern
        }
    }
}

/// An OAuth2 client registration. `google` and `microsoft` are built in:
/// they need only `client_id` (and Google a `client_secret` reference);
/// other providers also need both URLs.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct OAuthProviderConfig {
    /// Provider id referenced by [`AuthConfig::OAuth2::provider`].
    pub id: String,
    /// The registered client id.
    pub client_id: String,
    /// Keychain reference of the client secret, for providers that issue
    /// one to installed apps.
    #[serde(default)]
    pub client_secret: Option<SecretRef>,
    /// Authorization endpoint; `null` for built-in providers.
    #[serde(default)]
    pub auth_url: Option<String>,
    /// Token endpoint; `null` for built-in providers.
    #[serde(default)]
    pub token_url: Option<String>,
    /// Scopes to request; empty = the provider's mail scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
}

impl OAuthProviderConfig {
    /// Ids of the providers whose endpoints and scopes Nail knows.
    pub const BUILT_IN: [&'static str; 2] = ["google", "microsoft"];
}

// ---------------------------------------------------------------------------
// Account setup

/// A new secret handed to the core together with an account configuration
/// ([`crate::Request::AddAccount`], [`crate::Request::UpdateAccount`],
/// [`crate::Request::TestAccount`]). The core stores it in the keychain under
/// the reference of the matching endpoint and never sends it back.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SecretInput {
    /// Which endpoint's authentication the secret belongs to.
    pub endpoint: EndpointSlot,
    /// The secret.
    pub value: SecretValue,
}

/// Incoming or outgoing server of an account.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "lowercase")]
pub enum EndpointSlot {
    /// [`AccountConfig::incoming`].
    Incoming,
    /// [`AccountConfig::outgoing`].
    Outgoing,
}

/// The secret material of a [`SecretInput`].
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum SecretValue {
    /// A password typed by the user.
    Password {
        /// The password.
        password: String,
    },
    /// The tokens of a finished [`crate::Request::StartOAuth`] flow, held by
    /// the core.
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// The finished flow.
        flow_id: OAuthFlowId,
    },
    /// A PEM certificate chain with its private key.
    ClientCertificate {
        /// PEM text.
        pem: String,
    },
}

impl fmt::Debug for SecretValue {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Password { .. } => f.write_str("Password(<redacted>)"),
            Self::OAuth2 { flow_id } => write!(f, "OAuth2({flow_id})"),
            Self::ClientCertificate { .. } => f.write_str("ClientCertificate(<redacted>)"),
        }
    }
}

/// One ranked account-setup proposal from [`crate::Request::DiscoverAccount`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct DiscoveredAccount {
    /// Where the settings came from.
    pub source: DiscoverySource,
    /// Incoming server; auth has the username filled in and no secret.
    pub incoming: IncomingServer,
    /// Outgoing server, if found.
    #[serde(default)]
    pub outgoing: Option<OutgoingServer>,
    /// Provider display name from the source (e.g. "Example Mail"), if any.
    /// Untrusted text from the source.
    #[serde(default)]
    pub provider_name: Option<String>,
    /// Reasons to check the proposal before sending a password to it;
    /// empty for proposals from authenticated sources.
    #[serde(default)]
    pub warnings: Vec<DiscoveryWarning>,
}

/// Why a discovered proposal needs a second look (see
/// [`DiscoveredAccount::warnings`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoveryWarning {
    /// Fetched over plain HTTP: anyone on the network path could have
    /// forged it.
    InsecureSource,
    /// A server is neither under the address's domain nor under the domain
    /// of its mail exchanger (MX).
    ForeignHosts,
}

/// Source of discovered settings, most trusted first.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum DiscoverySource {
    /// `autoconfig.<domain>` served by the provider.
    Autoconfig,
    /// `<domain>/.well-known/autoconfig`.
    WellKnown,
    /// Mozilla's ISP database.
    Ispdb,
    /// RFC 6186 SRV records.
    Srv,
    /// Microsoft Autodiscover.
    Autodiscover,
    /// Common host names (`imap.<domain>`, `mail.<domain>`, …) that answered.
    Guess,
    /// Built into Nail for a known provider's API (M3b: Gmail through the
    /// Gmail API for `gmail.com`, `googlemail.com` and domains whose mail
    /// Google hosts).
    Provider,
}

/// Result of [`crate::Request::TestAccount`].
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountTestResult {
    /// The incoming server check.
    pub incoming: EndpointTestResult,
    /// The outgoing server check; `null` when no outgoing server is set.
    pub outgoing: Option<EndpointTestResult>,
}

/// Result of connecting and logging in to one server.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct EndpointTestResult {
    /// `null` when connecting and logging in worked; else the error class
    /// (`auth_failed`, `network`, `tls`, …).
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), `null` on success.
    pub message: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
    /// Capabilities / extensions the server announced (e.g. `IDLE`,
    /// `CONDSTORE`, `SIZE 35882577`), for display.
    pub capabilities: Vec<String>,
}

impl EndpointTestResult {
    /// Whether the check succeeded.
    pub fn ok(&self) -> bool {
        self.error.is_none()
    }
}

// ---------------------------------------------------------------------------
// Runtime status

/// Connection state of an account, shown by status indicators.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct AccountStatus {
    /// The account.
    pub account_id: AccountId,
    /// Current state.
    pub state: AccountState,
    /// When the state was entered (RFC 3339).
    pub since: String,
    /// Error class for `offline` / `auth-error`, else `null`.
    pub error: Option<ErrorCode>,
    /// Diagnostic detail (English), `null` when there is no error.
    pub message: Option<String>,
    /// The user-facing text (Fluent id + arguments) when the core has a more
    /// specific one than the error class; else `null` (ADR 0043).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(optional)]
    pub localized: Option<LocalizedMessage>,
}

/// The states of [`AccountStatus`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, TS)]
#[serde(rename_all = "kebab-case")]
pub enum AccountState {
    /// Connected (or last contact succeeded) and up to date.
    Online,
    /// Connecting or synchronizing.
    Connecting,
    /// Server unreachable; local changes queue up and replay later.
    Offline,
    /// The server rejected the credentials; the user must sign in again.
    AuthError,
    /// Disabled in the configuration.
    Disabled,
}

impl AccountState {
    /// All states.
    pub const ALL: [AccountState; 5] = [
        Self::Online,
        Self::Connecting,
        Self::Offline,
        Self::AuthError,
        Self::Disabled,
    ];

    /// Wire name, e.g. `auth-error`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Online => "online",
            Self::Connecting => "connecting",
            Self::Offline => "offline",
            Self::AuthError => "auth-error",
            Self::Disabled => "disabled",
        }
    }

    /// Fluent message id the UI shows, e.g. `account-state-auth-error`.
    pub fn message_id(self) -> String {
        format!("account-state-{}", self.as_str())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use pretty_assertions::assert_eq;
    use serde_json::json;

    #[test]
    fn sending_mailbox_from_headers() {
        let own = "ada@work.example";
        assert_eq!(
            SendingMailbox::of("ADA@work.example", None, own),
            SendingMailbox::Own
        );
        assert_eq!(
            SendingMailbox::of("team@work.example", None, own),
            SendingMailbox::SendAs {
                mailbox: "team@work.example".into()
            }
        );
        assert_eq!(
            SendingMailbox::of("team@work.example", Some("ada@work.example"), own),
            SendingMailbox::OnBehalf {
                mailbox: "team@work.example".into()
            }
        );
        // A Sender equal to From is not "on behalf".
        assert_eq!(
            SendingMailbox::of("team@work.example", Some("team@work.example"), own),
            SendingMailbox::SendAs {
                mailbox: "team@work.example".into()
            }
        );
        let send_as: SendAs = serde_json::from_value(json!({"mode": "on-behalf"})).unwrap();
        assert_eq!(send_as.mode, SendAsMode::OnBehalf);
        assert_eq!(send_as.sender, None);
        assert_eq!(SendAsMode::SendAs.as_str(), "send-as");
        assert_eq!(
            serde_json::to_value(SendAsMode::SendAs).unwrap(),
            json!("send-as")
        );
    }

    fn sample() -> AccountConfig {
        AccountConfig {
            id: "work".into(),
            name: "Work".into(),
            email: "ada@work.example".into(),
            enabled: true,
            incoming: IncomingServer::Imap {
                server: ServerEndpoint {
                    host: "imap.work.example".into(),
                    port: 993,
                    tls: TlsMode::Implicit,
                    auth: AuthConfig::Password {
                        username: "ada".into(),
                        secret: None,
                        mechanism: None,
                    },
                },
                options: ImapOptions::default(),
            },
            outgoing: Some(OutgoingServer::Smtp {
                server: ServerEndpoint {
                    host: "smtp.work.example".into(),
                    port: 465,
                    tls: TlsMode::Implicit,
                    auth: AuthConfig::OAuth2 {
                        username: "ada@work.example".into(),
                        provider: "google".into(),
                        secret: Some(SecretRef::parse("keychain:nail/work/oauth2").unwrap()),
                        mechanism: Some(SaslMechanism::Xoauth2),
                    },
                },
                options: SmtpOptions::default(),
            }),
            identities: vec![],
            sync: SyncPolicy::default(),
            save_sent_copy: None,
            authserv_ids: Vec::new(),
            sieve: None,
        }
    }

    #[test]
    fn account_config_json_shape() {
        let value = serde_json::to_value(sample()).unwrap();
        assert_eq!(value["incoming"]["protocol"], json!("imap"));
        assert_eq!(value["incoming"]["server"]["tls"], json!("implicit"));
        assert_eq!(
            value["incoming"]["server"]["auth"],
            json!({"method": "password", "username": "ada", "secret": null, "mechanism": null})
        );
        assert_eq!(
            value["outgoing"]["server"]["auth"]["method"],
            json!("oauth2")
        );
        assert_eq!(
            value["outgoing"]["server"]["auth"]["mechanism"],
            json!("xoauth2")
        );
        let back: AccountConfig = serde_json::from_value(value).unwrap();
        assert_eq!(back, sample());
        assert_eq!(back.kind(), crate::AccountKind::Imap);
    }

    #[test]
    fn missing_optional_fields_default() {
        let config: AccountConfig = serde_json::from_value(json!({
            "id": "p", "name": "P", "email": "p@example.org",
            "incoming": {"protocol": "pop3", "server": {
                "host": "pop.example.org", "port": 995, "tls": "implicit",
                "auth": {"method": "none"}
            }}
        }))
        .unwrap();
        assert!(config.enabled);
        assert_eq!(config.outgoing, None);
        assert_eq!(config.kind(), crate::AccountKind::Pop3);
        assert_eq!(config.incoming.server().auth, AuthConfig::None);
    }

    #[test]
    fn secret_refs() {
        let r = SecretRef::parse("keychain:nail/work/password").unwrap();
        assert_eq!((r.service(), r.name()), ("nail", "work/password"));
        assert_eq!(
            SecretRef::parse("hunter2").unwrap_err(),
            SecretRefError::Scheme
        );
        assert_eq!(
            SecretRef::parse("keychain:nail").unwrap_err(),
            SecretRefError::Shape
        );
        assert_eq!(
            SecretRef::parse("keychain:/x").unwrap_err(),
            SecretRefError::Shape
        );
        assert_eq!(
            SecretRef::parse("keychain:nail/a b").unwrap_err(),
            SecretRefError::Characters
        );
        assert_eq!(
            SecretRef::default_for(&"work".into(), SecretKind::OAuth2).as_str(),
            "keychain:nail/work/oauth2"
        );
        let auth = AuthConfig::Password {
            username: "u".into(),
            secret: None,
            mechanism: None,
        };
        assert_eq!(
            auth.secret_ref(&"home".into()).unwrap().as_str(),
            "keychain:nail/home/password"
        );
        assert_eq!(AuthConfig::None.secret_ref(&"home".into()), None);
    }

    #[test]
    fn exchange_accounts_and_windows_auth() {
        let integrated = AuthConfig::Windows {
            username: None,
            secret: None,
            package: None,
        };
        assert_eq!(integrated.secret_kind(), None);
        assert_eq!(integrated.secret_ref(&"work".into()), None);
        let explicit = AuthConfig::Windows {
            username: Some("CONTOSO\\ada".into()),
            secret: None,
            package: Some(SspiPackage::Ntlm),
        };
        assert_eq!(
            explicit.secret_ref(&"work".into()).unwrap().as_str(),
            "keychain:nail/work/password"
        );
        assert_eq!(
            serde_json::to_value(&explicit).unwrap(),
            json!({"method": "windows", "username": "CONTOSO\\ada", "secret": null, "package": "ntlm"})
        );
        assert_eq!(SspiPackage::parse("kerberos"), Some(SspiPackage::Kerberos));
        assert_eq!(SspiPackage::Ntlm.sspi_name(), "NTLM");
        let mut account = sample();
        account.outgoing = None;
        account.incoming = IncomingServer::Exchange {
            server: ServerEndpoint {
                host: "mail.contoso.example".into(),
                port: 443,
                tls: TlsMode::Implicit,
                auth: integrated,
            },
            options: crate::exchange::ExchangeOptions::for_engine(
                crate::exchange::ExchangeEngine::Ews,
            ),
        };
        assert_eq!(account.kind(), crate::AccountKind::Exchange);
        assert_eq!(account.incoming.protocol(), MailProtocol::Ews);
        assert!(account.can_send() && account.incoming.sends_itself());
        let value = serde_json::to_value(&account).unwrap();
        assert_eq!(value["incoming"]["protocol"], json!("exchange"));
        assert_eq!(value["incoming"]["options"]["engine"], json!("ews"));
        let back: AccountConfig = serde_json::from_value(value).unwrap();
        assert_eq!(back, account);
        assert_eq!(default_port(MailProtocol::Graph, TlsMode::Implicit), 443);
        assert_eq!(default_port(MailProtocol::Ews, TlsMode::Plaintext), 80);
    }

    #[test]
    fn secret_values_are_redacted_in_debug() {
        let v = SecretValue::Password {
            password: "hunter2".into(),
        };
        assert!(!format!("{v:?}").contains("hunter2"));
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            json!({"kind": "password", "password": "hunter2"})
        );
        let v = SecretValue::OAuth2 {
            flow_id: "f1".into(),
        };
        assert_eq!(
            serde_json::to_value(&v).unwrap(),
            json!({"kind": "oauth2", "flow_id": "f1"})
        );
    }

    #[test]
    fn ports_default_by_tls() {
        assert_eq!(default_port(MailProtocol::Imap, TlsMode::Implicit), 993);
        assert_eq!(default_port(MailProtocol::Imap, TlsMode::Starttls), 143);
        assert_eq!(default_port(MailProtocol::Pop3, TlsMode::Implicit), 995);
        assert_eq!(default_port(MailProtocol::Pop3, TlsMode::Plaintext), 110);
        assert_eq!(default_port(MailProtocol::Smtp, TlsMode::Implicit), 465);
        assert_eq!(default_port(MailProtocol::Smtp, TlsMode::Starttls), 587);
    }

    #[test]
    fn sync_policy_resolution() {
        let policy = SyncPolicy {
            bodies: Some(BodyPolicy::HeadersOnly),
            folders: vec![
                FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Inbox),
                    bodies: Some(BodyPolicy::Full),
                    enabled: None,
                    idle: None,
                },
                FolderPolicy {
                    folder: FolderMatch::Path("Lists/rust".into()),
                    bodies: Some(BodyPolicy::Recent { days: 30 }),
                    enabled: None,
                    idle: Some(true),
                },
                FolderPolicy {
                    folder: FolderMatch::Role(FolderRole::Junk),
                    bodies: None,
                    enabled: Some(false),
                    idle: None,
                },
            ],
        };
        let inbox = policy.effective("INBOX", FolderRole::Inbox);
        assert_eq!(
            (inbox.bodies, inbox.enabled, inbox.idle),
            (BodyPolicy::Full, true, true)
        );
        let list = policy.effective("Lists/rust", FolderRole::Other);
        assert_eq!(
            (list.bodies, list.idle),
            (BodyPolicy::Recent { days: 30 }, true)
        );
        assert!(!policy.effective("Spam", FolderRole::Junk).enabled);
        let other = policy.effective("Misc", FolderRole::Other);
        assert_eq!(
            (other.bodies, other.enabled, other.idle),
            (BodyPolicy::HeadersOnly, true, false)
        );
        assert!(FolderMatch::Path("INBOX".into()).matches("inbox", FolderRole::Other));
        assert!(!FolderMatch::Role(FolderRole::Other).matches("x", FolderRole::Other));
        assert_eq!(
            serde_json::to_value(FolderMatch::Role(FolderRole::Archive)).unwrap(),
            json!({"role": "archive"})
        );
        assert_eq!(
            serde_json::to_value(BodyPolicy::Recent { days: 7 }).unwrap(),
            json!({"mode": "recent", "days": 7})
        );
    }

    #[test]
    fn identity_rules_match_domains() {
        let rule = |p: &str| IdentityRule {
            pattern: p.into(),
            identity: "id".into(),
        };
        assert!(rule("*@client.com").matches("Bob@Client.com"));
        assert!(!rule("*@client.com").matches("bob@eu.client.com"));
        assert!(rule("*@*.client.com").matches("bob@eu.client.com"));
        assert!(!rule("*@*.client.com").matches("bob@client.com"));
        assert!(rule("boss@client.com").matches("BOSS@client.com"));
        assert!(!rule("boss@client.com").matches("bob@client.com"));
    }

    #[test]
    fn accounts_config_lookups() {
        let mut account = sample();
        account.identities.push(Identity {
            id: "ada@work.example".into(),
            account_id: "work".into(),
            email: "ada@work.example".into(),
            name: Some("Ada".into()),
            reply_to: None,
            bcc: vec![],
            signature: None,
            composer_mode: None,
            posting: None,
            mail_style: None,
            crypto: None,
            send_as: None,
        });
        let config = AccountsConfig {
            accounts: vec![account],
            ..AccountsConfig::default()
        };
        assert!(config.account(&"work".into()).is_some());
        assert!(config.virtual_account(&"work".into()).is_none());
        assert_eq!(
            config.identity(&"ada@work.example".into()).unwrap().name,
            Some("Ada".into())
        );
        assert_eq!(config.effective_undo_send_secs(), 10);
        assert_eq!(
            config.accounts[0].default_identity().unwrap().email,
            "ada@work.example"
        );
    }

    #[test]
    fn account_states_have_message_ids() {
        assert_eq!(
            AccountState::AuthError.message_id(),
            "account-state-auth-error"
        );
        for state in AccountState::ALL {
            assert_eq!(serde_json::to_value(state).unwrap(), json!(state.as_str()));
        }
    }
}
