//! JSON Schema of `accounts.kdl`.
//!
//! Like the theme and layout schemas, it describes the file as a JSON
//! object: each kind of top-level node is a property, each named node a map
//! key, each child node a property of its parent. The types below exist only
//! to generate the schema; the reader produces `nail_api` types directly.
//! `tests/accounts.rs` checks that every property the reader accepts is in
//! the schema.

#![allow(dead_code)] // Schema-only mirror types are never constructed.

use indexmap::IndexMap;
use schemars::{JsonSchema, Schema, generate::SchemaSettings};
use serde_json::Value;

/// JSON Schema of `accounts.kdl`.
pub fn accounts_schema() -> Schema {
    let mut schema = SchemaSettings::draft2020_12()
        .into_generator()
        .into_root_schema_for::<AccountsFileSchema>();
    schema.insert(
        "title".to_owned(),
        Value::String("Nail accounts.kdl".to_owned()),
    );
    schema
}

/// An `accounts.kdl` file.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct AccountsFileSchema {
    /// `undo-send-delay <seconds>`: default delay before a sent message
    /// really leaves (0 to 600, default 10).
    #[schemars(range(max = 600))]
    undo_send_delay: Option<u32>,
    /// `oauth-provider "<id>" client-id=… { scopes … }`: OAuth2 client
    /// registrations. `google` and `microsoft` are built in and need only
    /// `client-id`.
    #[serde(default)]
    oauth_provider: IndexMap<String, OAuthProviderSchema>,
    /// `account "<name>" email=… { … }`: real accounts, in display order.
    /// Names: lowercase letters, digits, `-`, `_`, `.`; `all` is reserved.
    #[serde(default)]
    account: IndexMap<String, AccountSchema>,
    /// `virtual-account "<name>" { members … }`: several accounts shown as
    /// one.
    #[serde(default)]
    virtual_account: IndexMap<String, VirtualAccountSchema>,
    /// `calendar-subscription "<name>" url=… { auth … }`: read-only ICS /
    /// webcal calendar subscriptions. Names: like account names, unique
    /// among calendar and contact sources; `local` is reserved.
    #[serde(default)]
    calendar_subscription: IndexMap<String, CalendarSubscriptionSchema>,
    /// `caldav "<name>" account=… url=… { auth … }`: CalDAV servers
    /// (calendars and task lists).
    #[serde(default)]
    caldav: IndexMap<String, DavSourceSchema>,
    /// `carddav "<name>" account=… url=… { auth … }`: CardDAV servers
    /// (address books).
    #[serde(default)]
    carddav: IndexMap<String, DavSourceSchema>,
}

/// `calendar-subscription "<name>" name=… url=… color=… alarms=… refresh=… enabled=… { auth … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct CalendarSubscriptionSchema {
    /// Display name (default: the calendar name of the feed).
    name: Option<String>,
    /// The feed: https:, webcal: or webcals: (http: only for localhost).
    url: String,
    /// Colour: #RGB, #RRGGBB, #RRGGBBAA or a CSS colour name.
    color: Option<String>,
    /// Fire the feed's own reminders (default #false).
    alarms: Option<bool>,
    /// Refresh interval in minutes, 5 to 10080 (default: the feed's
    /// suggestion, else 360).
    #[schemars(range(min = 5, max = 10080))]
    refresh: Option<u32>,
    /// `#false` stops refreshing without removing the calendar (default `#true`).
    enabled: Option<bool>,
    /// Login for protected feeds (no `username` default).
    auth: Option<AuthSchema>,
}

/// `caldav "<name>"` / `carddav "<name>"` `name=… account=… url=… refresh=… enabled=… { auth … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct DavSourceSchema {
    /// Display name (default: the server name).
    name: Option<String>,
    /// The mail account whose login this server uses (and whose email
    /// domain finds it when `url` is missing).
    account: Option<String>,
    /// Server URL, https: (http: only for localhost). Required without
    /// `account`; default: discovered from the account's email domain.
    url: Option<String>,
    /// Poll interval in minutes, 5 to 10080 (default 15).
    #[schemars(range(min = 5, max = 10080))]
    refresh: Option<u32>,
    /// `#false` stops syncing without removing the data (default `#true`).
    enabled: Option<bool>,
    /// Login (default: the account's incoming-server login; required
    /// without `account`, with a `username`). Secret default:
    /// `keychain:nail/pim/<name>/password`.
    auth: Option<AuthSchema>,
}

/// A keychain reference: `keychain:<service>/<name>`.
#[derive(JsonSchema)]
struct SecretRefSchema(#[schemars(pattern(r"^keychain:[^/\s]+/\S+$"))] String);

/// `oauth-provider "<id>" client-id=… client-secret=… auth-url=… token-url=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct OAuthProviderSchema {
    /// The registered client id.
    client_id: String,
    /// Keychain reference of the client secret, if the provider issued one.
    client_secret: Option<SecretRefSchema>,
    /// Authorization endpoint (required unless built in).
    auth_url: Option<String>,
    /// Token endpoint (required unless built in).
    token_url: Option<String>,
    /// `scopes "<scope>" …`: scopes to request instead of the mail defaults.
    #[serde(default)]
    scopes: Vec<String>,
}

/// `account "<name>" name=… email=… enabled=… save-sent-copy=… { … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct AccountSchema {
    /// Display name (default: the account name).
    name: Option<String>,
    /// Primary email address.
    email: String,
    /// `#false` stops syncing without removing the account (default `#true`).
    enabled: Option<bool>,
    /// Append sent mail to the Sent folder (default: automatic).
    save_sent_copy: Option<bool>,
    /// `imap host=… { auth … }`: IMAP incoming server (exactly one of imap
    /// and pop3).
    imap: Option<ImapSchema>,
    /// `pop3 host=… { auth … }`: POP3 incoming server.
    pop3: Option<Pop3Schema>,
    /// `gmail { auth "oauth2" provider="google" }`: the Gmail API (M3b;
    /// sends through the API, so no `smtp`).
    gmail: Option<GmailSchema>,
    /// `exchange engine="graph"|"ews" … { auth … }`: Microsoft Exchange
    /// (M3c; mail, calendars, contacts and tasks; sends through Exchange,
    /// so no `smtp`).
    exchange: Option<ExchangeSchema>,
    /// `smtp host=… { auth … }`: outgoing server.
    smtp: Option<SmtpSchema>,
    /// `identity "<address>" … { bcc …; signature … }`: sending identities,
    /// the first is the default (default: one for `email`).
    #[serde(default)]
    identity: Vec<IdentitySchema>,
    /// `sync bodies=… { folder … }`: what to synchronize.
    sync: Option<SyncSchema>,
    /// `authserv-id "mx.example.net" …`: the ids your receiving servers
    /// write into `Authentication-Results`; only their results are shown
    /// (default: learned from recent mail).
    authserv_id: Option<Vec<String>>,
    /// `sieve host=… port=… tls=… script=… enabled=…`: ManageSieve for
    /// server-side rules and vacation replies (IMAP accounts only; the
    /// login is the IMAP login).
    sieve: Option<SieveSchema>,
}

/// `sieve host=… port=… tls=… script=… enabled=…` (every property optional).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct SieveSchema {
    /// `#false` keeps the settings but runs every rule in Nail (default `#true`).
    enabled: Option<bool>,
    /// Host name or IP address (default: the IMAP host).
    host: Option<String>,
    /// Port (default 4190).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default starttls).
    tls: Option<TlsSchema>,
    /// Name of Nail's script on the server (default "nail").
    script: Option<String>,
}

/// Transport security.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum TlsSchema {
    /// TLS from the start (default).
    Implicit,
    /// STARTTLS / STLS upgrade.
    Starttls,
    /// No TLS (local test servers only).
    Plaintext,
}

/// `imap host=… port=… tls=… poll-interval=… compress=… max-connections=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct ImapSchema {
    /// Host name or IP address.
    host: String,
    /// Port (default 993 with implicit TLS, else 143).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default implicit).
    tls: Option<TlsSchema>,
    /// Seconds between polls of folders without IDLE (default 300).
    #[schemars(range(min = 30, max = 86400))]
    poll_interval: Option<u32>,
    /// Use COMPRESS=DEFLATE when offered (default `#true`).
    compress: Option<bool>,
    /// Maximum simultaneous connections (default 4).
    #[schemars(range(min = 1, max = 16))]
    max_connections: Option<u32>,
    /// How to log in (default: password, user name = account email).
    auth: Option<AuthSchema>,
}

/// `pop3 host=… port=… tls=… poll-interval=… leave-on-server=…
/// delete-after-days=… delete-when-deleted-locally=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct Pop3Schema {
    /// Host name or IP address.
    host: String,
    /// Port (default 995 with implicit TLS, else 110).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default implicit).
    tls: Option<TlsSchema>,
    /// Seconds between downloads (default 600).
    #[schemars(range(min = 30, max = 86400))]
    poll_interval: Option<u32>,
    /// Keep messages on the server (default `#true`).
    leave_on_server: Option<bool>,
    /// Delete from the server this many days after download.
    #[schemars(range(min = 1, max = 36500))]
    delete_after_days: Option<u32>,
    /// Delete from the server when deleted locally (default `#false`).
    delete_when_deleted_locally: Option<bool>,
    /// How to log in.
    auth: Option<AuthSchema>,
}

/// `gmail host=… port=… tls=… poll-interval=… max-poll-interval=…`
/// (every property optional).
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct GmailSchema {
    /// API host (default gmail.googleapis.com).
    host: Option<String>,
    /// Port (default 443).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default implicit; plaintext only for localhost
    /// test servers).
    tls: Option<TlsSchema>,
    /// Shortest seconds between history polls (default 30).
    #[schemars(range(min = 10, max = 86400))]
    poll_interval: Option<u32>,
    /// Longest seconds between history polls when nothing changes
    /// (default 300).
    #[schemars(range(min = 10, max = 86400))]
    max_poll_interval: Option<u32>,
    /// How to log in: `auth "oauth2" provider="google"`.
    auth: Option<AuthSchema>,
}

/// Which Exchange protocol an account speaks.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ExchangeEngineSchema {
    /// Microsoft Graph, for Exchange Online / Microsoft 365 (default).
    Graph,
    /// Exchange Web Services, for Exchange Server on-premises.
    Ews,
}

/// `exchange engine=… host=… port=… tls=… url=… poll-interval=…
/// max-poll-interval=… streaming=… server-rules=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct ExchangeSchema {
    /// graph (default) or ews.
    engine: Option<ExchangeEngineSchema>,
    /// Host (default graph.microsoft.com for Graph; EWS: the Exchange
    /// server, or the host of `url`).
    host: Option<String>,
    /// Port (default 443).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default implicit; plaintext only for localhost
    /// test servers; no starttls).
    tls: Option<TlsSchema>,
    /// EWS only: the endpoint Autodiscover found (default
    /// https://<host>/EWS/Exchange.asmx).
    url: Option<String>,
    /// Shortest seconds between change polls (default 30).
    #[schemars(range(min = 10, max = 86400))]
    poll_interval: Option<u32>,
    /// Longest seconds between change polls when nothing changes
    /// (default 300).
    #[schemars(range(min = 10, max = 86400))]
    max_poll_interval: Option<u32>,
    /// EWS only: push through streaming notifications (default `#true`).
    streaming: Option<bool>,
    /// Place Nail rules as Exchange inbox rules where possible (default
    /// `#true`).
    server_rules: Option<bool>,
    /// How to log in: Graph `auth "oauth2" provider="microsoft"`; EWS also
    /// `auth "windows"` or `auth "password"` (Basic over TLS).
    auth: Option<AuthSchema>,
}

/// `smtp host=… port=… tls=… dsn=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct SmtpSchema {
    /// Host name or IP address.
    host: String,
    /// Port (default 465 with implicit TLS, else 587).
    #[schemars(range(min = 1, max = 65535))]
    port: Option<u32>,
    /// Transport security (default implicit).
    tls: Option<TlsSchema>,
    /// Request delivery status notifications for failures.
    dsn: Option<bool>,
    /// How to log in.
    auth: Option<AuthSchema>,
}

/// `auth "<method>" …`.
#[derive(JsonSchema)]
#[serde(tag = "method", rename_all = "kebab-case")]
enum AuthSchema {
    /// `auth "password" username=… secret=… mechanism=…`.
    Password {
        /// Login name (default: the account email).
        username: Option<String>,
        /// Keychain reference (default `keychain:nail/<account>/password`).
        secret: Option<SecretRefSchema>,
        /// plain, login, scram-sha-256, or apop (POP3).
        mechanism: Option<String>,
    },
    /// `auth "oauth2" provider=… username=… secret=… mechanism=…`.
    #[serde(rename = "oauth2")]
    OAuth2 {
        /// google, microsoft or an oauth-provider id.
        provider: String,
        /// Login name (default: the account email).
        username: Option<String>,
        /// Keychain reference of the refresh token (default
        /// `keychain:nail/<account>/oauth2`).
        secret: Option<SecretRefSchema>,
        /// xoauth2 or oauthbearer.
        mechanism: Option<String>,
    },
    /// `auth "client-certificate" username=… secret=…`.
    ClientCertificate {
        /// Authorization identity, if needed.
        username: Option<String>,
        /// Keychain reference of the PEM certificate and key (default
        /// `keychain:nail/<account>/client-certificate`).
        secret: Option<SecretRefSchema>,
    },
    /// `auth "windows" username=… secret=… package=…`: Windows sign-in
    /// (Exchange only; Negotiate / Kerberos / NTLM through SSPI).
    Windows {
        /// `DOMAIN\user` or `user@domain` (default: the signed-in Windows
        /// user, no secret).
        username: Option<String>,
        /// Keychain reference of the password of `username` (default
        /// `keychain:nail/<account>/password`).
        secret: Option<SecretRefSchema>,
        /// negotiate (default), kerberos or ntlm.
        package: Option<String>,
    },
    /// `auth "none"`.
    None,
}

/// Composer mode.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum ComposerSchema {
    /// Plain text.
    Plain,
    /// HTML.
    Html,
    /// Markdown.
    Markdown,
}

/// `identity "<address>" id=… name=… reply-to=… composer=… posting=… send-as=… sender=… { bcc …; signature …; mail-style …; crypto … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct IdentitySchema {
    /// The sender address (the node's argument).
    email: String,
    /// Stable id when the same address is used twice (default: the address).
    id: Option<String>,
    /// Display name.
    name: Option<String>,
    /// Reply-To address.
    reply_to: Option<String>,
    /// Composer mode for new messages.
    composer: Option<ComposerSchema>,
    /// `bcc "<address>" …`: always-Bcc addresses.
    #[serde(default)]
    bcc: Vec<String>,
    /// `signature "<text>" html=… markdown=…`.
    signature: Option<SignatureSchema>,
    /// Quote placement in replies (default bottom).
    posting: Option<PostingSchema>,
    /// `mail-style css=… code-theme=…`: stylesheet and code theme of HTML /
    /// Markdown mail.
    mail_style: Option<MailStyleSchema>,
    /// `crypto …`: end-to-end encryption and signing defaults.
    crypto: Option<CryptoSchema>,
    /// Send for another mailbox (a shared / delegate Exchange mailbox whose
    /// address is the identity's): "send-as" or "on-behalf".
    send_as: Option<SendAsSchema>,
    /// With send-as="on-behalf": the user's own address for `Sender`
    /// (default: the account's address).
    sender: Option<String>,
}

/// How an identity sends for another mailbox.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum SendAsSchema {
    /// The mail is from the mailbox alone (Send As permission).
    SendAs,
    /// From the mailbox, on behalf of the user (Send on Behalf permission).
    OnBehalf,
}

/// `crypto technology=… sign=… encrypt=… openpgp-key=… smime-cert=…
/// autocrypt=… prefer-encrypt=… protect-headers=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct CryptoSchema {
    /// Preferred technology (default auto: what every recipient supports,
    /// OpenPGP when both work).
    technology: Option<TechnologySchema>,
    /// Sign new messages by default (default false).
    sign: Option<bool>,
    /// Encrypt new messages by default when every recipient has a key
    /// (default false).
    encrypt: Option<bool>,
    /// Own OpenPGP key: primary key fingerprint (40 or 64 hex digits).
    openpgp_key: Option<String>,
    /// Own S/MIME certificate: SHA-256 fingerprint (64 hex digits).
    smime_cert: Option<String>,
    /// Send Autocrypt headers and learn from received ones (default true).
    autocrypt: Option<bool>,
    /// Autocrypt prefer-encrypt (default nopreference).
    prefer_encrypt: Option<PreferEncryptSchema>,
    /// Hide the subject of encrypted mail on the wire (default true).
    protect_headers: Option<bool>,
}

/// End-to-end technology.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum TechnologySchema {
    /// Whatever every recipient supports.
    Auto,
    /// OpenPGP.
    Openpgp,
    /// S/MIME.
    Smime,
}

/// Autocrypt prefer-encrypt.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum PreferEncryptSchema {
    /// Encrypt by default when both sides say so.
    Mutual,
    /// No preference.
    Nopreference,
}

/// Quote placement in replies.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum PostingSchema {
    /// New text above the quote.
    Top,
    /// New text below the quote.
    Bottom,
}

/// `mail-style css=… code-theme=…`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct MailStyleSchema {
    /// CSS inlined into outgoing HTML (default: the built-in stylesheet).
    css: Option<String>,
    /// Code-highlighting theme of Markdown code blocks, e.g. `InspiredGitHub`.
    code_theme: Option<String>,
}

/// A signature.
#[derive(JsonSchema)]
struct SignatureSchema {
    /// Plain text (the node's argument).
    text: Option<String>,
    /// HTML variant.
    html: Option<String>,
    /// Markdown variant.
    markdown: Option<String>,
}

/// What to download. Not applied yet: this version always downloads whole
/// messages.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum BodiesSchema {
    /// Whole messages.
    Full,
    /// Headers; bodies on demand (not applied yet).
    HeadersOnly,
    /// Whole messages of the last `days` days (not applied yet).
    Recent,
}

/// `sync bodies=… days=… { folder … }`.
#[derive(JsonSchema)]
struct SyncSchema {
    /// Default for all folders (default full). Not applied yet: whole
    /// messages are always downloaded.
    bodies: Option<BodiesSchema>,
    /// Days, with bodies="recent" (not applied yet).
    #[schemars(range(min = 1, max = 36500))]
    days: Option<u32>,
    /// `folder path=…|role=… bodies=… days=… enabled=… idle=…` rules, later
    /// ones override earlier ones.
    #[serde(default)]
    folder: Vec<FolderRuleSchema>,
}

/// A special-use role.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
enum RoleSchema {
    /// Inbox.
    Inbox,
    /// Sent.
    Sent,
    /// Drafts.
    Drafts,
    /// Archive.
    Archive,
    /// Junk.
    Junk,
    /// Trash.
    Trash,
}

/// One folder rule.
#[derive(JsonSchema)]
struct FolderRuleSchema {
    /// Full server path (or use `role`).
    path: Option<String>,
    /// Special-use role (or use `path`).
    role: Option<RoleSchema>,
    /// What to download (not applied yet).
    bodies: Option<BodiesSchema>,
    /// Days, with bodies="recent" (not applied yet).
    #[schemars(range(min = 1, max = 36500))]
    days: Option<u32>,
    /// Synchronize the folder at all (default `#true`).
    enabled: Option<bool>,
    /// Watch with IDLE (default: inbox only).
    idle: Option<bool>,
}

/// `virtual-account "<name>" name=… default-identity=… merge-roles=… { … }`.
#[derive(JsonSchema)]
#[serde(rename_all = "kebab-case")]
struct VirtualAccountSchema {
    /// Display name (default: the name).
    name: Option<String>,
    /// Identity id for new messages (default: the first member's default).
    default_identity: Option<String>,
    /// Merge folders with the same role (default `#true`).
    merge_roles: Option<bool>,
    /// `members "<account>" …`: the real accounts shown together.
    #[schemars(length(min = 1))]
    members: Vec<String>,
    /// `map "<folder name>" { folder account=… path=… }`: merged folders.
    #[serde(default)]
    map: IndexMap<String, Vec<MappedFolderSchema>>,
    /// `identity-rule "<pattern>" identity=…`: identity by recipient domain.
    #[serde(default)]
    identity_rule: IndexMap<String, IdentityRuleSchema>,
}

/// `folder account=… path=…` inside `map`.
#[derive(JsonSchema)]
struct MappedFolderSchema {
    /// A member account.
    account: String,
    /// Full server path in that account.
    path: String,
}

/// `identity-rule "<pattern>" identity=…`.
#[derive(JsonSchema)]
struct IdentityRuleSchema {
    /// The identity id to use.
    identity: String,
}
