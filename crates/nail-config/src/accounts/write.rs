//! Writing `accounts.kdl`: a canonical renderer and a format-preserving
//! reconciler.
//!
//! [`update_text`] turns an existing file plus a desired [`AccountsConfig`]
//! into the new file text. Everything the change does not touch stays
//! byte-for-byte as it was, including comments, blank lines, indentation
//! and property order:
//!
//! * top-level nodes (accounts, virtual accounts, OAuth providers,
//!   `undo-send-delay`, PIM sources) are matched by kind and name; a node whose meaning
//!   did not change is not touched at all, even if it is written
//!   differently than the renderer would write it;
//! * a changed node is updated in place: properties and arguments whose
//!   value changed get the new value (keeping their position), removed ones
//!   disappear, new ones are appended; child nodes are matched by name (and
//!   key argument) and reconciled recursively;
//! * a removed node takes the comment block directly above it along, but
//!   not comments separated from it by a blank line;
//! * new nodes are appended after their predecessor in the desired order,
//!   using the file's indentation and line endings.

use std::collections::HashMap;
use std::fmt::Write as _;

use kdl::{KdlDocument, KdlEntry, KdlNode, KdlValue};
use nail_api::accounts::{
    AccountConfig, AccountsConfig, AuthConfig, BodyPolicy, ComposerMode, FolderMatch, FolderPolicy,
    IncomingServer, MailProtocol, OAuthProviderConfig, OutgoingServer, QuotePosting,
    ServerEndpoint, SyncPolicy, TlsMode, VirtualAccountConfig, default_port,
};
use nail_api::pim::{PimSourceConfig, PimSourceSettings};
use nail_api::security::{AutocryptPreference, CryptoPreference, IdentityCrypto};

use super::read::{mechanism_name, role_name};

/// Renders a complete file in the canonical layout.
pub(crate) fn render(config: &AccountsConfig, indent: &str) -> String {
    let mut blocks: Vec<String> = Vec::new();
    if let Some(secs) = config.undo_send_secs {
        blocks.push(render_undo(secs));
    }
    for provider in &config.oauth_providers {
        blocks.push(render_oauth_provider(provider, indent));
    }
    for account in &config.accounts {
        blocks.push(render_account(account, indent));
    }
    for virtual_account in &config.virtual_accounts {
        blocks.push(render_virtual_account(virtual_account, indent));
    }
    for source in &config.pim_sources {
        blocks.push(render_pim_source(source, indent));
    }
    blocks.join("\n")
}

/// Quotes a string as a KDL v2 quoted string.
pub(crate) fn quote(s: &str) -> String {
    let mut out = String::with_capacity(s.len() + 2);
    out.push('"');
    for c in s.chars() {
        match c {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            c if c.is_control()
                || matches!(
                    c,
                    '\u{200E}'
                        | '\u{200F}'
                        | '\u{202A}'..='\u{202E}'
                        | '\u{2066}'..='\u{2069}'
                        | '\u{FEFF}'
                ) =>
            {
                let _ = write!(out, "\\u{{{:x}}}", c as u32);
            }
            c => out.push(c),
        }
    }
    out.push('"');
    out
}

/// Accumulates one node line: `name arg… key=value…`.
struct Line(String);

impl Line {
    fn new(name: &str) -> Self {
        Self(name.to_owned())
    }

    fn arg(mut self, value: &str) -> Self {
        self.0.push(' ');
        self.0.push_str(&quote(value));
        self
    }

    fn raw_arg(mut self, value: &str) -> Self {
        self.0.push(' ');
        self.0.push_str(value);
        self
    }

    fn prop(mut self, key: &str, value: &str) -> Self {
        let _ = write!(self.0, " {key}={}", quote(value));
        self
    }

    fn prop_opt(self, key: &str, value: Option<&str>) -> Self {
        match value {
            Some(v) => self.prop(key, v),
            None => self,
        }
    }

    fn num(mut self, key: &str, value: impl std::fmt::Display) -> Self {
        let _ = write!(self.0, " {key}={value}");
        self
    }

    fn num_opt(self, key: &str, value: Option<u32>) -> Self {
        match value {
            Some(v) => self.num(key, v),
            None => self,
        }
    }

    fn bool_opt(mut self, key: &str, value: Option<bool>) -> Self {
        if let Some(b) = value {
            let _ = write!(self.0, " {key}=#{b}");
        }
        self
    }

    /// Finishes the node at `depth` with the given (already indented)
    /// children lines, or as a leaf when there are none.
    fn finish(self, depth: usize, indent: &str, children: &[String]) -> String {
        let pad = indent.repeat(depth);
        if children.is_empty() {
            format!("{pad}{}\n", self.0)
        } else {
            let mut out = format!("{pad}{} {{\n", self.0);
            for child in children {
                out.push_str(child);
            }
            out.push_str(&pad);
            out.push_str("}\n");
            out
        }
    }
}

fn tls_name(tls: TlsMode) -> &'static str {
    match tls {
        TlsMode::Implicit => "implicit",
        TlsMode::Starttls => "starttls",
        TlsMode::Plaintext => "plaintext",
    }
}

fn render_auth(auth: &AuthConfig, depth: usize, indent: &str) -> String {
    let line = match auth {
        AuthConfig::Password {
            username,
            secret,
            mechanism,
        } => Line::new("auth")
            .arg("password")
            .prop("username", username)
            .prop_opt("secret", secret.as_ref().map(|s| s.as_str()))
            .prop_opt("mechanism", mechanism.map(mechanism_name)),
        AuthConfig::OAuth2 {
            username,
            provider,
            secret,
            mechanism,
        } => Line::new("auth")
            .arg("oauth2")
            .prop("provider", provider)
            .prop("username", username)
            .prop_opt("secret", secret.as_ref().map(|s| s.as_str()))
            .prop_opt("mechanism", mechanism.map(mechanism_name)),
        AuthConfig::ClientCertificate { username, secret } => Line::new("auth")
            .arg("client-certificate")
            .prop_opt("username", username.as_deref())
            .prop_opt("secret", secret.as_ref().map(|s| s.as_str())),
        AuthConfig::None => Line::new("auth").arg("none"),
    };
    line.finish(depth, indent, &[])
}

fn endpoint_line(name: &str, server: &ServerEndpoint) -> Line {
    Line::new(name)
        .prop("host", &server.host)
        .num("port", server.port)
        .prop("tls", tls_name(server.tls))
}

fn render_account(account: &AccountConfig, indent: &str) -> String {
    let mut children = Vec::new();
    match &account.incoming {
        IncomingServer::Imap { server, options } => {
            let line = endpoint_line("imap", server)
                .num_opt("poll-interval", options.poll_interval_secs)
                .bool_opt("compress", options.compress)
                .num_opt("max-connections", options.max_connections);
            children.push(line.finish(1, indent, &[render_auth(&server.auth, 2, indent)]));
        }
        IncomingServer::Pop3 { server, options } => {
            let line = endpoint_line("pop3", server)
                .num_opt("poll-interval", options.poll_interval_secs)
                .bool_opt("leave-on-server", options.leave_on_server)
                .num_opt("delete-after-days", options.delete_after_days)
                .bool_opt(
                    "delete-when-deleted-locally",
                    options.delete_when_deleted_locally,
                );
            children.push(line.finish(1, indent, &[render_auth(&server.auth, 2, indent)]));
        }
        IncomingServer::Gmail { server, options } => {
            let line = endpoint_line("gmail", server)
                .num_opt("poll-interval", options.poll_interval_secs)
                .num_opt("max-poll-interval", options.max_poll_interval_secs);
            children.push(line.finish(1, indent, &[render_auth(&server.auth, 2, indent)]));
        }
    }
    if let Some(OutgoingServer::Smtp { server, options }) = &account.outgoing {
        let line = endpoint_line("smtp", server).bool_opt("dsn", options.dsn);
        children.push(line.finish(1, indent, &[render_auth(&server.auth, 2, indent)]));
    }
    for identity in &account.identities {
        let id = (identity.id.as_str() != identity.email).then_some(identity.id.as_str());
        let line = Line::new("identity")
            .arg(&identity.email)
            .prop_opt("id", id)
            .prop_opt("name", identity.name.as_deref())
            .prop_opt("reply-to", identity.reply_to.as_deref())
            .prop_opt(
                "composer",
                identity.composer_mode.map(|m| match m {
                    ComposerMode::Plain => "plain",
                    ComposerMode::Html => "html",
                    ComposerMode::Markdown => "markdown",
                }),
            )
            .prop_opt(
                "posting",
                identity.posting.map(|p| match p {
                    QuotePosting::Top => "top",
                    QuotePosting::Bottom => "bottom",
                }),
            );
        let mut grand = Vec::new();
        if !identity.bcc.is_empty() {
            let mut bcc = Line::new("bcc");
            for address in &identity.bcc {
                bcc = bcc.arg(address);
            }
            grand.push(bcc.finish(2, indent, &[]));
        }
        if let Some(signature) = &identity.signature {
            let mut sig = Line::new("signature");
            if let Some(text) = &signature.text {
                sig = sig.arg(text);
            }
            sig = sig
                .prop_opt("html", signature.html.as_deref())
                .prop_opt("markdown", signature.markdown.as_deref());
            grand.push(sig.finish(2, indent, &[]));
        }
        if let Some(style) = &identity.mail_style {
            let style = Line::new("mail-style")
                .prop_opt("css", style.css.as_deref())
                .prop_opt("code-theme", style.code_theme.as_deref());
            grand.push(style.finish(2, indent, &[]));
        }
        if let Some(crypto) = &identity.crypto {
            grand.push(render_identity_crypto(crypto).finish(2, indent, &[]));
        }
        children.push(line.finish(1, indent, &grand));
    }
    if account.sync != SyncPolicy::default() {
        children.push(render_sync(&account.sync, indent));
    }
    if let Some(sieve) = &account.sieve {
        let line = Line::new("sieve")
            .bool_opt("enabled", sieve.enabled)
            .prop_opt("host", sieve.host.as_deref())
            .num_opt("port", sieve.port.map(u32::from))
            .prop_opt("tls", sieve.tls.map(tls_name))
            .prop_opt("script", sieve.script_name.as_deref());
        children.push(line.finish(1, indent, &[]));
    }
    if !account.authserv_ids.is_empty() {
        let mut ids = Line::new("authserv-id");
        for id in &account.authserv_ids {
            ids = ids.arg(id);
        }
        children.push(ids.finish(1, indent, &[]));
    }
    let line = Line::new("account")
        .arg(account.id.as_str())
        .prop("name", &account.name)
        .prop("email", &account.email)
        .bool_opt("enabled", (!account.enabled).then_some(false))
        .bool_opt("save-sent-copy", account.save_sent_copy);
    line.finish(0, indent, &children)
}

/// The `crypto` node of an identity; only values that differ from the
/// defaults are written (an all-default node stays as a bare `crypto`).
fn render_identity_crypto(crypto: &IdentityCrypto) -> Line {
    let defaults = IdentityCrypto::default();
    let differs = |value: bool, default: bool| (value != default).then_some(value);
    Line::new("crypto")
        .prop_opt(
            "technology",
            (crypto.technology != defaults.technology).then_some(match crypto.technology {
                CryptoPreference::Auto => "auto",
                CryptoPreference::OpenPgp => "openpgp",
                CryptoPreference::Smime => "smime",
            }),
        )
        .bool_opt("sign", differs(crypto.sign, defaults.sign))
        .bool_opt("encrypt", differs(crypto.encrypt, defaults.encrypt))
        .prop_opt("openpgp-key", crypto.openpgp_key.as_deref())
        .prop_opt("smime-cert", crypto.smime_certificate.as_deref())
        .bool_opt("autocrypt", differs(crypto.autocrypt, defaults.autocrypt))
        .prop_opt(
            "prefer-encrypt",
            (crypto.prefer_encrypt != defaults.prefer_encrypt).then_some(
                match crypto.prefer_encrypt {
                    AutocryptPreference::Mutual => "mutual",
                    AutocryptPreference::NoPreference => "nopreference",
                },
            ),
        )
        .bool_opt(
            "protect-headers",
            differs(crypto.protect_headers, defaults.protect_headers),
        )
}

fn body_props(line: Line, bodies: Option<BodyPolicy>) -> Line {
    match bodies {
        None => line,
        Some(BodyPolicy::Full) => line.prop("bodies", "full"),
        Some(BodyPolicy::HeadersOnly) => line.prop("bodies", "headers-only"),
        Some(BodyPolicy::Recent { days }) => line.prop("bodies", "recent").num("days", days),
    }
}

fn render_sync(sync: &SyncPolicy, indent: &str) -> String {
    let rules: Vec<String> = sync
        .folders
        .iter()
        .map(|rule: &FolderPolicy| {
            let line = match &rule.folder {
                FolderMatch::Path(path) => Line::new("folder").prop("path", path),
                FolderMatch::Role(role) => Line::new("folder").prop("role", role_name(*role)),
            };
            body_props(line, rule.bodies)
                .bool_opt("enabled", rule.enabled)
                .bool_opt("idle", rule.idle)
                .finish(2, indent, &[])
        })
        .collect();
    body_props(Line::new("sync"), sync.bodies).finish(1, indent, &rules)
}

fn render_virtual_account(v: &VirtualAccountConfig, indent: &str) -> String {
    let mut children = Vec::new();
    let mut members = Line::new("members");
    for member in &v.members {
        members = members.arg(member.as_str());
    }
    children.push(members.finish(1, indent, &[]));
    for mapping in &v.folder_mappings {
        let folders: Vec<String> = mapping
            .sources
            .iter()
            .map(|s| {
                Line::new("folder")
                    .prop("account", s.account_id.as_str())
                    .prop("path", &s.path)
                    .finish(2, indent, &[])
            })
            .collect();
        children.push(
            Line::new("map")
                .arg(&mapping.name)
                .finish(1, indent, &folders),
        );
    }
    for rule in &v.identity_rules {
        children.push(
            Line::new("identity-rule")
                .arg(&rule.pattern)
                .prop("identity", rule.identity.as_str())
                .finish(1, indent, &[]),
        );
    }
    Line::new("virtual-account")
        .arg(v.id.as_str())
        .prop("name", &v.name)
        .prop_opt(
            "default-identity",
            v.default_identity.as_ref().map(|i| i.as_str()),
        )
        .bool_opt("merge-roles", (!v.merge_roles).then_some(false))
        .finish(0, indent, &children)
}

fn render_oauth_provider(p: &OAuthProviderConfig, indent: &str) -> String {
    let mut children = Vec::new();
    if !p.scopes.is_empty() {
        let mut scopes = Line::new("scopes");
        for scope in &p.scopes {
            scopes = scopes.arg(scope);
        }
        children.push(scopes.finish(1, indent, &[]));
    }
    Line::new("oauth-provider")
        .arg(&p.id)
        .prop("client-id", &p.client_id)
        .prop_opt(
            "client-secret",
            p.client_secret.as_ref().map(|s| s.as_str()),
        )
        .prop_opt("auth-url", p.auth_url.as_deref())
        .prop_opt("token-url", p.token_url.as_deref())
        .finish(0, indent, &children)
}

/// Renders a `calendar-subscription`, `caldav` or `carddav` node (M3a).
fn render_pim_source(source: &PimSourceConfig, indent: &str) -> String {
    let node = source
        .kind()
        .node_name()
        .expect("configured sources have a node name");
    let mut line = Line::new(node).arg(source.id.as_str());
    let auth = match &source.settings {
        PimSourceSettings::Ics {
            url,
            color,
            alarms,
            refresh_minutes,
            auth,
        } => {
            line = line
                .prop_opt("name", source.name.as_deref())
                .prop("url", url)
                .prop_opt("color", color.as_deref())
                .bool_opt("alarms", alarms.then_some(true))
                .num_opt("refresh", *refresh_minutes);
            auth
        }
        PimSourceSettings::Caldav {
            account,
            url,
            auth,
            refresh_minutes,
        }
        | PimSourceSettings::Carddav {
            account,
            url,
            auth,
            refresh_minutes,
        } => {
            line = line
                .prop_opt("name", source.name.as_deref())
                .prop_opt("account", account.as_ref().map(|a| a.as_str()))
                .prop_opt("url", url.as_deref())
                .num_opt("refresh", *refresh_minutes);
            auth
        }
    };
    let line = line.bool_opt("enabled", (!source.enabled).then_some(false));
    let children: Vec<String> = auth.iter().map(|a| render_auth(a, 1, indent)).collect();
    line.finish(0, indent, &children)
}

/// Renders a single undo-send node (used when the file gains one).
pub(crate) fn render_undo(secs: u32) -> String {
    Line::new("undo-send-delay")
        .raw_arg(&secs.to_string())
        .finish(0, "", &[])
}

// ---------------------------------------------------------------------------
// Reconciling

/// The indentation unit of a file: the leading whitespace of its first
/// indented line, else four spaces.
pub(crate) fn detect_indent(text: &str) -> String {
    for line in text.lines() {
        let ws: String = line
            .chars()
            .take_while(|c| *c == ' ' || *c == '\t')
            .collect();
        if !ws.is_empty() && line.len() > ws.len() && !line[ws.len()..].starts_with("//") {
            return ws;
        }
    }
    "    ".to_owned()
}

pub(crate) fn uses_crlf(text: &str) -> bool {
    text.contains("\r\n")
}

/// Which top-level node a key names, for the semantic equality check.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
enum TopKey {
    Account(String),
    Virtual(String),
    Provider(String),
    Undo,
    /// A PIM source: node name and id.
    Pim(String, String),
    Other(String),
}

fn top_key(node: &KdlNode) -> TopKey {
    let arg0 = node
        .entries()
        .iter()
        .find(|e| e.name().is_none())
        .and_then(|e| e.value().as_string())
        .unwrap_or_default()
        .to_owned();
    match node.name().value() {
        "account" => TopKey::Account(arg0),
        "virtual-account" => TopKey::Virtual(arg0),
        "oauth-provider" => TopKey::Provider(arg0),
        "undo-send-delay" => TopKey::Undo,
        kind @ ("calendar-subscription" | "caldav" | "carddav") => {
            TopKey::Pim(kind.to_owned(), arg0)
        }
        other => TopKey::Other(format!("{other}:{arg0}")),
    }
}

/// Child nodes that occur at most once per parent and are matched by name
/// alone, so that changing their arguments updates them in place.
const SINGLETONS: &[&str] = &[
    "imap",
    "pop3",
    "smtp",
    "sync",
    "auth",
    "members",
    "bcc",
    "signature",
    "mail-style",
    "crypto",
    "scopes",
    "authserv-id",
    "sieve",
];

/// Properties that identify a child among siblings of the same name.
const KEY_PROPS: &[&str] = &["id", "path", "role", "account"];

fn child_key(node: &KdlNode) -> String {
    let name = node.name().value();
    if SINGLETONS.contains(&name) {
        return name.to_owned();
    }
    let mut key = name.to_owned();
    if let Some(arg) = node.entries().iter().find(|e| e.name().is_none()) {
        key.push('\u{1}');
        key.push_str(&arg.value().to_string());
    }
    for prop in KEY_PROPS {
        if let Some(v) = node.get(*prop) {
            key.push('\u{1}');
            key.push_str(prop);
            key.push('=');
            key.push_str(&v.to_string());
        }
    }
    key
}

/// Structural equality ignoring formatting: name, type, arguments in order,
/// properties as a map (last one wins, as in KDL), children in order.
fn node_eq(a: &KdlNode, b: &KdlNode) -> bool {
    fn props(n: &KdlNode) -> HashMap<&str, &KdlValue> {
        n.entries()
            .iter()
            .filter_map(|e| e.name().map(|k| (k.value(), e.value())))
            .collect()
    }
    fn args(n: &KdlNode) -> Vec<&KdlValue> {
        n.entries()
            .iter()
            .filter(|e| e.name().is_none())
            .map(KdlEntry::value)
            .collect()
    }
    fn kids(n: &KdlNode) -> &[KdlNode] {
        n.children().map_or(&[], KdlDocument::nodes)
    }
    a.name().value() == b.name().value()
        && a.ty().map(|t| t.value()) == b.ty().map(|t| t.value())
        && args(a) == args(b)
        && props(a) == props(b)
        && kids(a).len() == kids(b).len()
        && kids(a).iter().zip(kids(b)).all(|(x, y)| node_eq(x, y))
}

/// Moves the line break that follows `{` from the first child's leading
/// whitespace into the children document's own leading format, so that
/// every child's leading is just "(blank lines / comments) + indentation".
fn hoist(doc: &mut KdlDocument, top_level: bool) {
    if !top_level {
        let moved = doc.nodes_mut().first_mut().and_then(|first| {
            let fmt = first.format_mut()?;
            let cut = if fmt.leading.starts_with("\r\n") {
                2
            } else if fmt.leading.starts_with('\n') {
                1
            } else {
                return None;
            };
            let head: String = fmt.leading.drain(..cut).collect();
            Some(head)
        });
        if let Some(head) = moved {
            let mut fmt = doc.format().cloned().unwrap_or_default();
            fmt.leading.push_str(&head);
            doc.set_format(fmt);
        }
    }
    for node in doc.nodes_mut() {
        if let Some(children) = node.children_mut() {
            hoist(children, false);
        }
    }
}

/// Splits a removed node's leading text into the part that stays in the
/// file (everything up to the last blank line) and the part that goes with
/// the node (the comment block directly above it).
pub(crate) fn split_leading(leading: &str) -> (&str, &str) {
    let bytes = leading.as_bytes();
    let mut keep_until = 0;
    let mut line_start = 0;
    for (i, &b) in bytes.iter().enumerate() {
        if b == b'\n' {
            let line = &leading[line_start..i];
            if line.trim().is_empty() && i > 0 && line_start > 0 {
                // A whitespace-only line that is preceded by another line
                // break: a blank line. Keep everything up to it.
                keep_until = i + 1;
            }
            line_start = i + 1;
        }
    }
    leading.split_at(keep_until)
}

struct Ctx<'a> {
    newline: &'a str,
    /// The `email` of the account being reconciled (the default user name).
    account_email: Option<&'a str>,
}

fn str_prop<'n>(node: &'n KdlNode, key: &str) -> Option<&'n str> {
    node.get(key).and_then(KdlValue::as_string)
}

fn first_arg(node: &KdlNode) -> Option<&KdlValue> {
    node.entries()
        .iter()
        .find(|e| e.name().is_none())
        .map(KdlEntry::value)
}

/// Whether property `key` with `value` on a node like `node` means the same
/// as leaving it out, i.e. the reader would fill in exactly this value.
/// Such properties are neither added to nor removed from a file by an edit.
fn is_default_prop(node: &KdlNode, key: &str, value: &KdlValue, ctx: &Ctx<'_>) -> bool {
    let protocol = match node.name().value() {
        "imap" => Some(MailProtocol::Imap),
        "pop3" => Some(MailProtocol::Pop3),
        "smtp" => Some(MailProtocol::Smtp),
        "gmail" => Some(MailProtocol::Gmail),
        _ => None,
    };
    match (node.name().value(), key) {
        ("gmail", "host") => value.as_string() == Some(nail_api::gmail::GMAIL_API_HOST),
        (_, "tls") if protocol.is_some() => value.as_string() == Some("implicit"),
        (_, "port") if protocol.is_some() => {
            let tls = match str_prop(node, "tls") {
                Some("starttls") => TlsMode::Starttls,
                Some("plaintext") => TlsMode::Plaintext,
                _ => TlsMode::Implicit,
            };
            let default = default_port(protocol.unwrap_or(MailProtocol::Imap), tls);
            value.as_integer() == Some(i128::from(default))
        }
        ("auth", "username") => {
            ctx.account_email.is_some() && value.as_string() == ctx.account_email
        }
        ("account" | "virtual-account", "name") => first_arg(node) == Some(value),
        ("account" | "calendar-subscription" | "caldav" | "carddav", "enabled")
        | ("virtual-account", "merge-roles") => value == &KdlValue::Bool(true),
        ("calendar-subscription", "alarms") => value == &KdlValue::Bool(false),
        _ => false,
    }
}

/// Whether a child node the renderer wrote is what the reader assumes when
/// the child is missing: the password login as the account's address, or
/// the single identity built from the account's address.
fn is_default_child(parent: &KdlNode, child: &KdlNode, ctx: &Ctx<'_>) -> bool {
    let no_kids = child.children().is_none_or(|d| d.nodes().is_empty());
    let props: Vec<&str> = child
        .entries()
        .iter()
        .filter_map(|e| e.name().map(|n| n.value()))
        .collect();
    let arg_count = child
        .entries()
        .iter()
        .filter(|e| e.name().is_none())
        .count();
    match (parent.name().value(), child.name().value()) {
        ("imap" | "pop3" | "smtp", "auth") => {
            no_kids
                && arg_count == 1
                && first_arg(child).and_then(KdlValue::as_string) == Some("password")
                && props.iter().all(|p| *p == "username")
                && str_prop(child, "username").is_none_or(|u| Some(u) == ctx.account_email)
        }
        ("account", "identity") => {
            let identities = parent.children().map_or(0, |d| {
                d.nodes()
                    .iter()
                    .filter(|n| n.name().value() == "identity")
                    .count()
            });
            identities == 1
                && no_kids
                && props.is_empty()
                && arg_count == 1
                && first_arg(child).and_then(KdlValue::as_string) == ctx.account_email
        }
        _ => false,
    }
}

fn ensure_terminated(node: &mut KdlNode, newline: &str) {
    if let Some(fmt) = node.format_mut() {
        if fmt.terminator.is_empty() && !fmt.trailing.contains('\n') {
            fmt.terminator = newline.to_owned();
        }
    } else {
        node.set_format(kdl::KdlNodeFormat {
            terminator: newline.to_owned(),
            ..Default::default()
        });
    }
}

/// Updates `old`'s entries to match `new`'s values, keeping the
/// formatting of entries that stay.
fn reconcile_entries(old: &mut KdlNode, new: &KdlNode, ctx: &Ctx<'_>) {
    #[derive(PartialEq, Eq, Clone, Debug)]
    enum Key {
        Arg(usize),
        Prop(String),
    }
    fn keyed(entries: &[KdlEntry]) -> Vec<(Key, &KdlEntry)> {
        let mut index = 0;
        entries
            .iter()
            .map(|e| match e.name() {
                Some(name) => (Key::Prop(name.value().to_owned()), e),
                None => {
                    index += 1;
                    (Key::Arg(index - 1), e)
                }
            })
            .collect()
    }
    let new_entries = keyed(new.entries());
    let old_entries: Vec<(Key, KdlEntry)> = keyed(old.entries())
        .into_iter()
        .map(|(k, e)| (k, e.clone()))
        .collect();
    let mut result: Vec<KdlEntry> = Vec::new();
    let mut used: Vec<Key> = Vec::new();
    for (key, entry) in old_entries {
        if used.contains(&key) {
            continue; // a duplicate property; the new value is written once
        }
        let Some((_, wanted)) = new_entries.iter().find(|(k, _)| *k == key) else {
            // Gone from the desired node: keep it only if it spells out the
            // default anyway.
            if let Key::Prop(name) = &key
                && is_default_prop(new, name, entry.value(), ctx)
            {
                used.push(key);
                result.push(entry);
            }
            continue;
        };
        used.push(key);
        if entry.value() == wanted.value() && entry.ty() == wanted.ty() {
            result.push(entry);
        } else {
            let mut replacement = (*wanted).clone();
            if let (Some(old_fmt), Some(new_fmt)) = (entry.format(), replacement.format_mut()) {
                new_fmt.leading.clone_from(&old_fmt.leading);
                new_fmt.trailing.clone_from(&old_fmt.trailing);
            }
            result.push(replacement);
        }
    }
    for (key, entry) in &new_entries {
        let default = match key {
            Key::Prop(name) => is_default_prop(new, name, entry.value(), ctx),
            Key::Arg(_) => false,
        };
        if !used.contains(key) && !default {
            let mut e = (*entry).clone();
            if let Some(fmt) = e.format_mut() {
                // Rendered entries carry a single leading space.
                if fmt.leading.is_empty() {
                    fmt.leading.push(' ');
                }
            }
            result.push(e);
        }
    }
    // Keep arguments before properties when they were appended out of
    // order (KDL allows either; this reads better).
    let (mut arguments, properties): (Vec<KdlEntry>, Vec<KdlEntry>) =
        result.into_iter().partition(|e| e.name().is_none());
    arguments.extend(properties);
    *old.entries_mut() = arguments;
}

fn reconcile_node(old: &mut KdlNode, new: &KdlNode, ctx: &Ctx<'_>) {
    if node_eq(old, new) {
        return;
    }
    let account_ctx;
    let ctx = if new.name().value() == "account" {
        account_ctx = Ctx {
            newline: ctx.newline,
            account_email: str_prop(new, "email"),
        };
        &account_ctx
    } else {
        ctx
    };
    reconcile_entries(old, new, ctx);
    let new_kids = new.children().filter(|d| !d.nodes().is_empty());
    match (old.children().is_some(), new_kids) {
        (_, None) => {
            if old.children().is_some() {
                old.clear_children();
                if let Some(fmt) = old.format_mut() {
                    fmt.before_children.clear();
                }
            }
        }
        (false, Some(kids)) => {
            let mut kids = kids.clone();
            kids.nodes_mut()
                .retain(|child| !is_default_child(new, child, ctx));
            if !kids.nodes().is_empty() {
                old.set_children(kids);
                if let Some(fmt) = old.format_mut() {
                    fmt.before_children = " ".to_owned();
                }
            }
        }
        (true, Some(kids)) => {
            let old_kids = old.children_mut().as_mut().expect("checked above");
            reconcile_list(old_kids, kids, ctx, &|_, _| false, Some(new));
        }
    }
}

/// Reconciles the nodes of `old` with `new`. `same(old, new)` may declare a
/// matched pair equal although their text differs (semantic equality).
fn reconcile_list(
    old: &mut KdlDocument,
    new: &KdlDocument,
    ctx: &Ctx<'_>,
    same: &dyn Fn(&KdlNode, &KdlNode) -> bool,
    parent: Option<&KdlNode>,
) {
    let top_level = parent.is_none();
    let key = |n: &KdlNode| {
        if top_level {
            format!("{:?}", top_key(n))
        } else {
            child_key(n)
        }
    };
    let new_keys: Vec<String> = new.nodes().iter().map(key).collect();
    let old_nodes = std::mem::take(old.nodes_mut());
    let old_keys: Vec<String> = old_nodes.iter().map(key).collect();

    // Match each new node to the first unmatched old node with its key.
    let mut old_match: Vec<Option<usize>> = vec![None; old_nodes.len()];
    let mut new_matched = vec![false; new.nodes().len()];
    for (j, k) in new_keys.iter().enumerate() {
        if let Some(i) = (0..old_nodes.len()).find(|&i| old_match[i].is_none() && &old_keys[i] == k)
        {
            old_match[i] = Some(j);
            new_matched[j] = true;
        }
    }

    // At the top of the file, comments before the first node that are not
    // directly attached to it (or the whole block when there is no blank
    // line) are the file header: they stay at the top whatever happens to
    // the node.
    let mut old_nodes = old_nodes;
    if top_level && let Some(fmt) = old_nodes.first_mut().and_then(KdlNode::format_mut) {
        let (keep, attached) = split_leading(&fmt.leading);
        let (header, rest) = if keep.is_empty() {
            (fmt.leading.clone(), String::new())
        } else {
            (keep.to_owned(), attached.to_owned())
        };
        fmt.leading = rest;
        let mut doc_fmt = old.format().cloned().unwrap_or_default();
        doc_fmt.leading.push_str(&header);
        old.set_format(doc_fmt);
    }

    // Surviving old nodes, reconciled; removed ones hand the part of their
    // leading text that is not attached to them to the next survivor (in
    // file order), or to the end of the block.
    let mut survivors: Vec<Option<KdlNode>> = (0..new.nodes().len()).map(|_| None).collect();
    let mut carry = String::new();
    for (i, mut node) in old_nodes.into_iter().enumerate() {
        match old_match[i] {
            Some(j) => {
                let wanted = &new.nodes()[j];
                if !same(&node, wanted) {
                    reconcile_node(&mut node, wanted, ctx);
                }
                if !carry.is_empty()
                    && let Some(fmt) = node.format_mut()
                {
                    fmt.leading.insert_str(0, &carry);
                    carry.clear();
                }
                survivors[j] = Some(node);
            }
            None => {
                if let Some(fmt) = node.format() {
                    carry.push_str(split_leading(&fmt.leading).0);
                }
            }
        }
    }
    if !carry.is_empty() {
        let mut fmt = old.format().cloned().unwrap_or_default();
        fmt.trailing.insert_str(0, &carry);
        old.set_format(fmt);
    }
    if top_level
        && let Some(fmt) = old.format()
        && !fmt.leading.is_empty()
        && !fmt.leading.ends_with('\n')
    {
        let mut fmt = fmt.clone();
        fmt.leading.push_str(ctx.newline);
        old.set_format(fmt);
    }

    // The result follows the desired order: survivors (reconciled, with
    // their own comments) and new nodes where they belong.
    let mut result: Vec<(Option<usize>, KdlNode)> = Vec::new();
    for (j, node) in new.nodes().iter().enumerate() {
        if let Some(survivor) = survivors[j].take() {
            result.push((Some(j), survivor));
            continue;
        }
        if new_matched[j] || parent.is_some_and(|p| is_default_child(p, node, ctx)) {
            continue;
        }
        let mut node = node.clone();
        if let Some(fmt) = node.format_mut() {
            if top_level {
                fmt.leading = if result.is_empty() {
                    String::new()
                } else {
                    ctx.newline.to_owned()
                };
            }
            fmt.terminator = ctx.newline.to_owned();
            fmt.trailing.clear();
        }
        result.push((Some(j), node));
    }

    let count = result.len();
    let mut nodes: Vec<KdlNode> = result.into_iter().map(|(_, n)| n).collect();
    for node in nodes.iter_mut().take(count.saturating_sub(1)) {
        ensure_terminated(node, ctx.newline);
    }
    // A multi-line block whose first node lost its line break after `{`.
    if !top_level && let Some(first) = nodes.first_mut() {
        let doc_leading_has_break = old.format().is_some_and(|f| f.leading.contains('\n'));
        if let Some(fmt) = first.format_mut()
            && !doc_leading_has_break
            && !fmt.leading.contains('\n')
            && count > 1
        {
            fmt.leading.insert_str(0, ctx.newline);
        }
    }
    *old.nodes_mut() = nodes;
}

/// Parses rendered text into a document with hoisted leading breaks.
fn parse_rendered(text: &str, top_level: bool) -> KdlDocument {
    let mut doc = KdlDocument::parse_v2(text)
        .unwrap_or_else(|e| panic!("the accounts renderer produced invalid KDL ({e}):\n{text}"));
    hoist(&mut doc, top_level);
    doc
}

/// Rewrites `old_text` (a valid `accounts.kdl` whose meaning is `old`) so
/// that it means `new`, preserving everything unchanged. Returns the new
/// text; the caller validates it.
pub(crate) fn update_text(old_text: &str, old: &AccountsConfig, new: &AccountsConfig) -> String {
    let indent = detect_indent(old_text);
    let crlf = uses_crlf(old_text);
    let newline = if crlf { "\r\n" } else { "\n" };
    let mut rendered = render(new, &indent);
    if crlf {
        rendered = rendered.replace('\n', "\r\n");
    }
    let Ok(mut old_doc) = KdlDocument::parse_v2(old_text) else {
        return rendered;
    };
    hoist(&mut old_doc, true);
    let new_doc = parse_rendered(&rendered, true);

    let same = |a: &KdlNode, b: &KdlNode| -> bool {
        match (top_key(a), top_key(b)) {
            (TopKey::Account(x), TopKey::Account(y)) if x == y => {
                let id = x.as_str().into();
                old.account(&id).is_some() && old.account(&id) == new.account(&id)
            }
            (TopKey::Virtual(x), TopKey::Virtual(y)) if x == y => {
                let id = x.as_str().into();
                old.virtual_account(&id).is_some()
                    && old.virtual_account(&id) == new.virtual_account(&id)
            }
            (TopKey::Provider(x), TopKey::Provider(y)) if x == y => {
                let find =
                    |c: &AccountsConfig| c.oauth_providers.iter().find(|p| p.id == x).cloned();
                find(old).is_some() && find(old) == find(new)
            }
            (TopKey::Undo, TopKey::Undo) => old.undo_send_secs == new.undo_send_secs,
            (TopKey::Pim(kx, x), TopKey::Pim(ky, y)) if kx == ky && x == y => {
                let id = x.as_str().into();
                old.pim_source(&id).is_some() && old.pim_source(&id) == new.pim_source(&id)
            }
            _ => false,
        }
    };
    let ctx = Ctx {
        newline,
        account_email: None,
    };
    reconcile_list(&mut old_doc, &new_doc, &ctx, &same, None);
    let mut out = old_doc.to_string();
    if !out.is_empty() && !out.ends_with('\n') {
        out.push_str(newline);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quoting_escapes_what_kdl_needs() {
        assert_eq!(quote("plain"), "\"plain\"");
        assert_eq!(quote("a \"b\" \\ c"), "\"a \\\"b\\\" \\\\ c\"");
        assert_eq!(quote("line\nnext\ttab"), "\"line\\nnext\\ttab\"");
        assert_eq!(quote("\u{feff}x\u{7}"), "\"\\u{feff}x\\u{7}\"");
        for s in ["a\"b", "x\\y", "l1\nl2", "\u{202e}rtl", "tab\t", "é😀"] {
            let doc = KdlDocument::parse_v2(&format!("n {}", quote(s))).unwrap();
            assert_eq!(doc.nodes()[0].entries()[0].value().as_string(), Some(s));
        }
    }

    #[test]
    fn leading_split_keeps_paragraphs_above_a_blank_line() {
        assert_eq!(split_leading(""), ("", ""));
        assert_eq!(split_leading("    "), ("", "    "));
        assert_eq!(
            split_leading("// header\n\n// about x\n"),
            ("// header\n\n", "// about x\n")
        );
        assert_eq!(
            split_leading("    // about x\n    "),
            ("", "    // about x\n    ")
        );
        assert_eq!(
            split_leading("\n    // a\n\n    // b\n    "),
            ("\n    // a\n\n", "    // b\n    ")
        );
    }

    #[test]
    fn indentation_is_detected() {
        assert_eq!(detect_indent("a {\n  b\n}\n"), "  ");
        assert_eq!(detect_indent("a {\n\tb\n}\n"), "\t");
        assert_eq!(detect_indent("a\n"), "    ");
        assert_eq!(detect_indent("a {\n // x\n    b\n}\n"), "    ");
    }

    #[test]
    fn structural_equality_ignores_formatting() {
        let a = KdlDocument::parse_v2("n 1 x=\"a\"   y=2 {\n  c\n}\n").unwrap();
        let b = KdlDocument::parse_v2("n   1 y=2 x=a { c; }").unwrap();
        assert!(node_eq(&a.nodes()[0], &b.nodes()[0]));
        let c = KdlDocument::parse_v2("n 1 x=\"a\" y=3 { c }").unwrap();
        assert!(!node_eq(&a.nodes()[0], &c.nodes()[0]));
    }

    #[test]
    fn child_keys() {
        let doc = KdlDocument::parse_v2(
            "imap host=\"a\"\nidentity \"a@b\" id=\"x\"\nfolder role=\"junk\" enabled=#false\nmembers \"a\" \"b\"\n",
        )
        .unwrap();
        let keys: Vec<String> = doc.nodes().iter().map(child_key).collect();
        assert_eq!(keys[0], "imap");
        assert!(keys[1].starts_with("identity\u{1}"), "{}", keys[1]);
        assert!(
            keys[1].contains("a@b") && keys[1].contains("id="),
            "{}",
            keys[1]
        );
        assert!(
            keys[2].contains("role=") && keys[2].contains("junk"),
            "{}",
            keys[2]
        );
        assert_eq!(keys[3], "members");
        assert_eq!(render_undo(5), "undo-send-delay 5\n");
    }
}
