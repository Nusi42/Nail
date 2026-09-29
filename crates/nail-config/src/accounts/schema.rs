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
    /// `smtp host=… { auth … }`: outgoing server.
    smtp: Option<SmtpSchema>,
    /// `identity "<address>" … { bcc …; signature … }`: sending identities,
    /// the first is the default (default: one for `email`).
    #[serde(default)]
    identity: Vec<IdentitySchema>,
    /// `sync bodies=… { folder … }`: what to synchronize.
    sync: Option<SyncSchema>,
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

/// `identity "<address>" id=… name=… reply-to=… composer=… posting=… { bcc …; signature …; mail-style … }`.
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
