//! Reading `accounts.kdl` into [`AccountsConfig`], validating every node,
//! property and cross reference with precise positions.

use std::collections::{HashMap, HashSet};

use kdl::{KdlEntry, KdlNode, KdlValue};
use nail_api::accounts::{
    AccountConfig, AccountsConfig, AuthConfig, BodyPolicy, ComposerMode, FolderMapping,
    FolderMatch, FolderPolicy, FolderSource, Identity, IdentityRule, ImapOptions, IncomingServer,
    MailProtocol, OAuthProviderConfig, OutgoingServer, Pop3Options, SaslMechanism, SecretRef,
    ServerEndpoint, Signature, SmtpOptions, SyncPolicy, TlsMode, VirtualAccountConfig,
    default_port,
};
use nail_api::{AccountId, FolderRole, IdentityId};

use super::{
    MAX_UNDO_SEND_SECS, MIN_POLL_INTERVAL_SECS, is_valid_account_id, is_valid_identity_pattern,
};
use crate::error::ConfigError;
use crate::kdl_util::{
    Source, arg_str, args, check_max_args, check_no_children, check_props, children, describe,
    name, prop_str, props,
};

/// Property names that would put a secret into the file. They are refused
/// with a hint to the keychain instead of a generic "unknown property".
const SECRET_PROPS: &[&str] = &[
    "password",
    "pass",
    "token",
    "access-token",
    "refresh-token",
    "private-key",
];

pub(crate) fn read(src: &Source<'_>) -> Result<AccountsConfig, ConfigError> {
    let doc = src.parse()?;
    let reader = Reader { src };
    let mut config = AccountsConfig::default();
    let mut ids: HashSet<String> = HashSet::new();
    let mut identity_ids: HashMap<IdentityId, AccountId> = HashMap::new();
    let mut provider_ids: HashSet<String> = HashSet::new();
    let mut undo_seen = false;
    let mut virtual_nodes = Vec::new();
    let mut oauth_refs: Vec<(String, &KdlNode)> = Vec::new();

    for node in doc.nodes() {
        match name(node) {
            "account" => {
                let (account, provider_refs) = reader.account(node)?;
                if !ids.insert(account.id.to_string()) {
                    return Err(src.node_err(
                        node,
                        format!(
                            "an account named {:?} is already defined",
                            account.id.as_str()
                        ),
                    ));
                }
                let identity_nodes: Vec<&KdlNode> = children(node)
                    .iter()
                    .filter(|c| name(c) == "identity")
                    .collect();
                for (index, identity) in account.identities.iter().enumerate() {
                    if identity_ids
                        .insert(identity.id.clone(), account.id.clone())
                        .is_some()
                    {
                        return Err(src.node_err(
                            identity_nodes.get(index).copied().unwrap_or(node),
                            format!(
                                "identity id {:?} is used twice; give one of them a different `id=`",
                                identity.id.as_str()
                            ),
                        ));
                    }
                }
                oauth_refs.extend(provider_refs.into_iter().map(|p| (p, node)));
                config.accounts.push(account);
            }
            "virtual-account" => virtual_nodes.push(node),
            "oauth-provider" => {
                let provider = reader.oauth_provider(node)?;
                if !provider_ids.insert(provider.id.clone()) {
                    return Err(src.node_err(
                        node,
                        format!("OAuth provider {:?} is defined twice", provider.id),
                    ));
                }
                config.oauth_providers.push(provider);
            }
            "undo-send-delay" => {
                if undo_seen {
                    return Err(src.node_err(node, "`undo-send-delay` is set twice"));
                }
                undo_seen = true;
                config.undo_send_secs = Some(reader.undo_send_delay(node)?);
            }
            other => {
                return Err(src.node_err(
                    node,
                    format!(
                        "unknown node `{other}` (expected account, virtual-account, oauth-provider or undo-send-delay)"
                    ),
                ));
            }
        }
    }

    for (provider, node) in oauth_refs {
        let built_in = OAuthProviderConfig::BUILT_IN.contains(&provider.as_str());
        if !built_in && !provider_ids.contains(&provider) {
            return Err(src.node_err(
                node,
                format!(
                    "unknown OAuth provider {provider:?} (built in: google, microsoft; others need an `oauth-provider` node)"
                ),
            ));
        }
    }

    for node in virtual_nodes {
        let virtual_account = reader.virtual_account(node, &config, &identity_ids)?;
        if !ids.insert(virtual_account.id.to_string()) {
            return Err(src.node_err(
                node,
                format!(
                    "the name {:?} is already used by another account or virtual account",
                    virtual_account.id.as_str()
                ),
            ));
        }
        config.virtual_accounts.push(virtual_account);
    }
    // Virtual accounts keep their file order relative to each other; they
    // are read after the real accounts only to resolve references.
    Ok(config)
}

struct Reader<'s, 'a> {
    src: &'s Source<'a>,
}

impl Reader<'_, '_> {
    fn check_props(&self, node: &KdlNode, allowed: &[&str]) -> Result<(), ConfigError> {
        for (key, entry) in props(node) {
            if SECRET_PROPS.contains(&key) {
                return Err(self.src.entry_err(
                    entry,
                    format!(
                        "`{key}` is not allowed: secrets are never stored in accounts.kdl. Keep them in the OS keychain and reference them with secret=\"keychain:nail/<account>/password\""
                    ),
                ));
            }
        }
        check_props(self.src, node, allowed)
    }

    fn check_unique_props(&self, node: &KdlNode) -> Result<(), ConfigError> {
        let mut seen = HashSet::new();
        for (key, entry) in props(node) {
            if !seen.insert(key) {
                return Err(self
                    .src
                    .entry_err(entry, format!("property `{key}` is set twice")));
            }
        }
        Ok(())
    }

    fn prop_string(&self, node: &KdlNode, key: &str) -> Result<Option<String>, ConfigError> {
        Ok(prop_str(self.src, node, key)?.map(|(s, _)| s.to_owned()))
    }

    fn prop_nonempty(&self, node: &KdlNode, key: &str) -> Result<Option<String>, ConfigError> {
        match prop_str(self.src, node, key)? {
            Some((s, entry)) if s.trim().is_empty() => Err(self
                .src
                .entry_err(entry, format!("`{key}` must not be empty"))),
            other => Ok(other.map(|(s, _)| s.to_owned())),
        }
    }

    fn prop_bool(&self, node: &KdlNode, key: &str) -> Result<Option<bool>, ConfigError> {
        match node.entry(key) {
            None => Ok(None),
            Some(entry) => match entry.value() {
                KdlValue::Bool(b) => Ok(Some(*b)),
                other => Err(self.src.entry_err(
                    entry,
                    format!("`{key}` must be #true or #false, found {}", describe(other)),
                )),
            },
        }
    }

    fn prop_u32_range(
        &self,
        node: &KdlNode,
        key: &str,
        min: u32,
        max: u32,
    ) -> Result<Option<u32>, ConfigError> {
        let Some(entry) = node.entry(key) else {
            return Ok(None);
        };
        let value = entry
            .value()
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .filter(|v| (min..=max).contains(v));
        value.map(Some).ok_or_else(|| {
            self.src.entry_err(
                entry,
                format!(
                    "`{key}` must be a whole number from {min} to {max}, found {}",
                    describe(entry.value())
                ),
            )
        })
    }

    fn prop_secret(&self, node: &KdlNode, key: &str) -> Result<Option<SecretRef>, ConfigError> {
        match prop_str(self.src, node, key)? {
            None => Ok(None),
            Some((text, entry)) => SecretRef::parse(text)
                .map(Some)
                .map_err(|e| self.src.entry_err(entry, format!("`{key}`: {e}"))),
        }
    }

    fn prop_url(&self, node: &KdlNode, key: &str) -> Result<Option<String>, ConfigError> {
        match prop_str(self.src, node, key)? {
            None => Ok(None),
            Some((text, entry)) => {
                if is_allowed_url(text) {
                    Ok(Some(text.to_owned()))
                } else {
                    Err(self.src.entry_err(
                        entry,
                        format!(
                            "`{key}` must be an https:// URL (http:// only for localhost), found {text:?}"
                        ),
                    ))
                }
            }
        }
    }

    fn id_arg(&self, node: &KdlNode, what: &str) -> Result<AccountId, ConfigError> {
        let (id, entry) = arg_str(self.src, node, 0, what)?;
        if id == AccountId::ALL {
            return Err(self.src.entry_err(
                entry,
                format!(
                    "{:?} is reserved for the built-in \"All accounts\" virtual account",
                    AccountId::ALL
                ),
            ));
        }
        if !is_valid_account_id(id) {
            return Err(self.src.entry_err(
                entry,
                format!(
                    "invalid name {id:?}: use 1 to 64 lowercase letters, digits, `-`, `_` or `.`, starting with a letter or digit"
                ),
            ));
        }
        Ok(AccountId::new(id))
    }

    fn email(&self, entry: &KdlEntry, text: &str, what: &str) -> Result<String, ConfigError> {
        if is_plausible_email(text) {
            Ok(text.to_owned())
        } else {
            Err(self.src.entry_err(
                entry,
                format!(
                    "{what} must be an email address like \"name@example.org\", found {text:?}"
                ),
            ))
        }
    }

    // --- account ---------------------------------------------------------

    /// Reads an `account` node; also returns the OAuth provider ids it uses.
    fn account(&self, node: &KdlNode) -> Result<(AccountConfig, Vec<String>), ConfigError> {
        self.check_props(node, &["name", "email", "enabled", "save-sent-copy"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let id = self.id_arg(node, "the account name")?;
        let email = match prop_str(self.src, node, "email")? {
            Some((text, entry)) => self.email(entry, text, "`email`")?,
            None => {
                return Err(self
                    .src
                    .node_err(node, format!("account {:?} needs email=\"…\"", id.as_str())));
            }
        };
        let display = self
            .prop_nonempty(node, "name")?
            .unwrap_or_else(|| id.to_string());
        let enabled = self.prop_bool(node, "enabled")?.unwrap_or(true);
        let save_sent_copy = self.prop_bool(node, "save-sent-copy")?;

        let mut incoming: Option<IncomingServer> = None;
        let mut outgoing: Option<OutgoingServer> = None;
        let mut identities = Vec::new();
        let mut sync: Option<SyncPolicy> = None;
        let mut providers = Vec::new();
        for child in children(node) {
            match name(child) {
                protocol @ ("imap" | "pop3") => {
                    if incoming.is_some() {
                        return Err(self.src.node_err(
                            child,
                            "an account has exactly one incoming server (`imap` or `pop3`)",
                        ));
                    }
                    let (server, provider) = self.server(child, &email)?;
                    providers.extend(provider);
                    incoming = Some(if protocol == "imap" {
                        IncomingServer::Imap {
                            server,
                            options: self.imap_options(child)?,
                        }
                    } else {
                        IncomingServer::Pop3 {
                            server,
                            options: self.pop3_options(child)?,
                        }
                    });
                }
                "smtp" => {
                    if outgoing.is_some() {
                        return Err(self
                            .src
                            .node_err(child, "an account has at most one `smtp` server"));
                    }
                    let (server, provider) = self.server(child, &email)?;
                    providers.extend(provider);
                    outgoing = Some(OutgoingServer::Smtp {
                        server,
                        options: SmtpOptions {
                            dsn: self.prop_bool(child, "dsn")?,
                        },
                    });
                }
                "identity" => identities.push(self.identity(child, &id)?),
                "sync" => {
                    if sync.is_some() {
                        return Err(self.src.node_err(child, "`sync` is set twice"));
                    }
                    sync = Some(self.sync(child)?);
                }
                other => {
                    return Err(self.src.node_err(
                        child,
                        format!(
                            "unknown node `{other}` in `account` (expected imap, pop3, smtp, identity or sync)"
                        ),
                    ));
                }
            }
        }
        let incoming = incoming.ok_or_else(|| {
            self.src.node_err(
                node,
                format!(
                    "account {:?} needs an incoming server: an `imap` or `pop3` node",
                    id.as_str()
                ),
            )
        })?;
        if identities.is_empty() {
            identities.push(Identity {
                id: IdentityId::new(email.clone()),
                account_id: id.clone(),
                email: email.clone(),
                name: None,
                reply_to: None,
                bcc: Vec::new(),
                signature: None,
                composer_mode: None,
            });
        }
        let mut seen = HashSet::new();
        for (identity, child) in identities
            .iter()
            .zip(children(node).iter().filter(|c| name(c) == "identity"))
        {
            if !seen.insert(identity.id.clone()) {
                return Err(self.src.node_err(
                    child,
                    format!(
                        "identity id {:?} is used twice; give one of them a different `id=`",
                        identity.id.as_str()
                    ),
                ));
            }
        }
        Ok((
            AccountConfig {
                id,
                name: display,
                email,
                enabled,
                incoming,
                outgoing,
                identities,
                sync: sync.unwrap_or_default(),
                save_sent_copy,
            },
            providers,
        ))
    }

    /// Reads `imap` / `pop3` / `smtp`: the endpoint (and the OAuth provider
    /// it references, if any).
    fn server(
        &self,
        node: &KdlNode,
        account_email: &str,
    ) -> Result<(ServerEndpoint, Option<String>), ConfigError> {
        let protocol = match name(node) {
            "imap" => MailProtocol::Imap,
            "pop3" => MailProtocol::Pop3,
            _ => MailProtocol::Smtp,
        };
        let allowed: &[&str] = match protocol {
            MailProtocol::Imap => &[
                "host",
                "port",
                "tls",
                "poll-interval",
                "compress",
                "max-connections",
            ],
            MailProtocol::Pop3 => &[
                "host",
                "port",
                "tls",
                "poll-interval",
                "leave-on-server",
                "delete-after-days",
                "delete-when-deleted-locally",
            ],
            MailProtocol::Smtp => &["host", "port", "tls", "dsn"],
        };
        self.check_props(node, allowed)?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 0)?;
        let host = match prop_str(self.src, node, "host")? {
            Some((h, entry)) => {
                if h.is_empty()
                    || h.chars().any(|c| c.is_whitespace() || c.is_control())
                    || h.contains('/')
                {
                    return Err(self.src.entry_err(
                        entry,
                        format!("`host` must be a host name or IP address, found {h:?}"),
                    ));
                }
                h.to_owned()
            }
            None => {
                return Err(self
                    .src
                    .node_err(node, format!("`{}` needs host=\"…\"", name(node))));
            }
        };
        let tls = match prop_str(self.src, node, "tls")? {
            None => TlsMode::Implicit,
            Some((t, entry)) => {
                match t {
                    "implicit" => TlsMode::Implicit,
                    "starttls" => TlsMode::Starttls,
                    "plaintext" => TlsMode::Plaintext,
                    _ => {
                        return Err(self.src.entry_err(
                        entry,
                        format!("`tls` must be \"implicit\", \"starttls\" or \"plaintext\", found {t:?}"),
                    ));
                    }
                }
            }
        };
        let port = match self.prop_u32_range(node, "port", 1, 65535)? {
            Some(p) => u16::try_from(p).unwrap_or(u16::MAX),
            None => default_port(protocol, tls),
        };
        let mut auth: Option<(AuthConfig, Option<String>)> = None;
        for child in children(node) {
            if name(child) != "auth" {
                return Err(self.src.node_err(
                    child,
                    format!(
                        "unknown node `{}` in `{}` (expected auth)",
                        name(child),
                        name(node)
                    ),
                ));
            }
            if auth.is_some() {
                return Err(self.src.node_err(child, "`auth` is set twice"));
            }
            auth = Some(self.auth(child, protocol, account_email)?);
        }
        let (auth, provider) = auth.unwrap_or_else(|| {
            (
                AuthConfig::Password {
                    username: account_email.to_owned(),
                    secret: None,
                    mechanism: None,
                },
                None,
            )
        });
        Ok((
            ServerEndpoint {
                host,
                port,
                tls,
                auth,
            },
            provider,
        ))
    }

    fn auth(
        &self,
        node: &KdlNode,
        protocol: MailProtocol,
        account_email: &str,
    ) -> Result<(AuthConfig, Option<String>), ConfigError> {
        check_max_args(self.src, node, 1)?;
        check_no_children(self.src, node)?;
        self.check_unique_props(node)?;
        let (method, method_entry) = arg_str(self.src, node, 0, "the method")?;
        let username = |this: &Self| -> Result<String, ConfigError> {
            Ok(this
                .prop_nonempty(node, "username")?
                .unwrap_or_else(|| account_email.to_owned()))
        };
        let mechanism = |this: &Self,
                         allowed: &[SaslMechanism]|
         -> Result<Option<SaslMechanism>, ConfigError> {
            let Some((text, entry)) = prop_str(this.src, node, "mechanism")? else {
                return Ok(None);
            };
            let parsed = parse_mechanism(text).filter(|m| allowed.contains(m));
            parsed.map(Some).ok_or_else(|| {
                let names: Vec<&str> = allowed.iter().map(|m| mechanism_name(*m)).collect();
                this.src.entry_err(
                    entry,
                    format!(
                        "`mechanism` must be one of {} here, found {text:?}",
                        names.join(", ")
                    ),
                )
            })
        };
        Ok(match method {
            "password" => {
                self.check_props(node, &["username", "secret", "mechanism"])?;
                let mut allowed = vec![
                    SaslMechanism::Plain,
                    SaslMechanism::Login,
                    SaslMechanism::ScramSha256,
                ];
                if protocol == MailProtocol::Pop3 {
                    allowed.push(SaslMechanism::Apop);
                }
                (
                    AuthConfig::Password {
                        username: username(self)?,
                        secret: self.prop_secret(node, "secret")?,
                        mechanism: mechanism(self, &allowed)?,
                    },
                    None,
                )
            }
            "oauth2" => {
                self.check_props(node, &["username", "provider", "secret", "mechanism"])?;
                let provider = match prop_str(self.src, node, "provider")? {
                    Some((p, _)) => p.to_owned(),
                    None => {
                        return Err(self.src.node_err(
                            node,
                            "`auth \"oauth2\"` needs provider=\"google\", \"microsoft\" or the id of an `oauth-provider`",
                        ));
                    }
                };
                (
                    AuthConfig::OAuth2 {
                        username: username(self)?,
                        provider: provider.clone(),
                        secret: self.prop_secret(node, "secret")?,
                        mechanism: mechanism(
                            self,
                            &[SaslMechanism::Xoauth2, SaslMechanism::Oauthbearer],
                        )?,
                    },
                    Some(provider),
                )
            }
            "client-certificate" => {
                self.check_props(node, &["username", "secret"])?;
                (
                    AuthConfig::ClientCertificate {
                        username: self.prop_nonempty(node, "username")?,
                        secret: self.prop_secret(node, "secret")?,
                    },
                    None,
                )
            }
            "none" => {
                self.check_props(node, &[])?;
                (AuthConfig::None, None)
            }
            other => {
                return Err(self.src.entry_err(
                    method_entry,
                    format!(
                        "unknown auth method {other:?} (expected \"password\", \"oauth2\", \"client-certificate\" or \"none\")"
                    ),
                ));
            }
        })
    }

    fn imap_options(&self, node: &KdlNode) -> Result<ImapOptions, ConfigError> {
        Ok(ImapOptions {
            poll_interval_secs: self.prop_u32_range(
                node,
                "poll-interval",
                MIN_POLL_INTERVAL_SECS,
                86_400,
            )?,
            compress: self.prop_bool(node, "compress")?,
            max_connections: self.prop_u32_range(node, "max-connections", 1, 16)?,
        })
    }

    fn pop3_options(&self, node: &KdlNode) -> Result<Pop3Options, ConfigError> {
        let options = Pop3Options {
            poll_interval_secs: self.prop_u32_range(
                node,
                "poll-interval",
                MIN_POLL_INTERVAL_SECS,
                86_400,
            )?,
            leave_on_server: self.prop_bool(node, "leave-on-server")?,
            delete_after_days: self.prop_u32_range(node, "delete-after-days", 1, 36_500)?,
            delete_when_deleted_locally: self.prop_bool(node, "delete-when-deleted-locally")?,
        };
        if options.leave_on_server == Some(false) {
            for key in ["delete-after-days", "delete-when-deleted-locally"] {
                if let Some(entry) = node.entry(key) {
                    return Err(self.src.entry_err(
                        entry,
                        format!("`{key}` only applies with leave-on-server=#true"),
                    ));
                }
            }
        }
        Ok(options)
    }

    fn identity(&self, node: &KdlNode, account: &AccountId) -> Result<Identity, ConfigError> {
        self.check_props(node, &["id", "name", "reply-to", "composer"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let (email, entry) = arg_str(self.src, node, 0, "the sender address")?;
        let email = self.email(entry, email, "the identity")?;
        let id = match prop_str(self.src, node, "id")? {
            Some((id, entry)) => {
                if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(self
                        .src
                        .entry_err(entry, "`id` must be non-empty and contain no spaces"));
                }
                IdentityId::new(id)
            }
            None => IdentityId::new(email.clone()),
        };
        let reply_to = match prop_str(self.src, node, "reply-to")? {
            Some((text, entry)) => Some(self.email(entry, text, "`reply-to`")?),
            None => None,
        };
        let composer_mode = match prop_str(self.src, node, "composer")? {
            None => None,
            Some((text, entry)) => {
                Some(match text {
                    "plain" => ComposerMode::Plain,
                    "html" => ComposerMode::Html,
                    "markdown" => ComposerMode::Markdown,
                    _ => {
                        return Err(self.src.entry_err(
                        entry,
                        format!("`composer` must be \"plain\", \"html\" or \"markdown\", found {text:?}"),
                    ));
                    }
                })
            }
        };
        let mut bcc = Vec::new();
        let mut signature: Option<Signature> = None;
        for child in children(node) {
            match name(child) {
                "bcc" => {
                    self.check_props(child, &[])?;
                    check_no_children(self.src, child)?;
                    let list = args(child);
                    if list.is_empty() {
                        return Err(self.src.node_err(child, "`bcc` needs at least one address"));
                    }
                    for entry in list {
                        let text = crate::kdl_util::entry_str(self.src, entry, "a bcc address")?;
                        bcc.push(self.email(entry, text, "a bcc address")?);
                    }
                }
                "signature" => {
                    if signature.is_some() {
                        return Err(self.src.node_err(child, "`signature` is set twice"));
                    }
                    self.check_props(child, &["html", "markdown"])?;
                    self.check_unique_props(child)?;
                    check_max_args(self.src, child, 1)?;
                    check_no_children(self.src, child)?;
                    let text = match args(child).first() {
                        Some(entry) => Some(
                            crate::kdl_util::entry_str(self.src, entry, "the signature text")?
                                .to_owned(),
                        ),
                        None => None,
                    };
                    signature = Some(Signature {
                        text,
                        html: self.prop_string(child, "html")?,
                        markdown: self.prop_string(child, "markdown")?,
                    });
                }
                other => {
                    return Err(self.src.node_err(
                        child,
                        format!("unknown node `{other}` in `identity` (expected bcc or signature)"),
                    ));
                }
            }
        }
        Ok(Identity {
            id,
            account_id: account.clone(),
            email,
            name: self.prop_nonempty(node, "name")?,
            reply_to,
            bcc,
            signature,
            composer_mode,
        })
    }

    fn body_policy(&self, node: &KdlNode) -> Result<Option<BodyPolicy>, ConfigError> {
        let days = self.prop_u32_range(node, "days", 1, 36_500)?;
        let policy = match prop_str(self.src, node, "bodies")? {
            None => None,
            Some((text, entry)) => Some(match text {
                "full" => BodyPolicy::Full,
                "headers-only" => BodyPolicy::HeadersOnly,
                "recent" => BodyPolicy::Recent {
                    days: days.ok_or_else(|| {
                        self.src
                            .entry_err(entry, "bodies=\"recent\" needs days=<number of days>")
                    })?,
                },
                _ => {
                    return Err(self.src.entry_err(
                        entry,
                        format!(
                            "`bodies` must be \"full\", \"headers-only\" or \"recent\", found {text:?}"
                        ),
                    ));
                }
            }),
        };
        if !matches!(policy, Some(BodyPolicy::Recent { .. }))
            && let Some(entry) = node.entry("days")
        {
            return Err(self
                .src
                .entry_err(entry, "`days` only applies with bodies=\"recent\""));
        }
        Ok(policy)
    }

    fn sync(&self, node: &KdlNode) -> Result<SyncPolicy, ConfigError> {
        self.check_props(node, &["bodies", "days"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 0)?;
        let bodies = self.body_policy(node)?;
        let mut folders = Vec::new();
        for child in children(node) {
            if name(child) != "folder" {
                return Err(self.src.node_err(
                    child,
                    format!("unknown node `{}` in `sync` (expected folder)", name(child)),
                ));
            }
            self.check_props(
                child,
                &["path", "role", "bodies", "days", "enabled", "idle"],
            )?;
            self.check_unique_props(child)?;
            check_max_args(self.src, child, 0)?;
            check_no_children(self.src, child)?;
            let folder = self.folder_match(child)?;
            let rule = FolderPolicy {
                folder,
                bodies: self.body_policy(child)?,
                enabled: self.prop_bool(child, "enabled")?,
                idle: self.prop_bool(child, "idle")?,
            };
            if rule.bodies.is_none() && rule.enabled.is_none() && rule.idle.is_none() {
                return Err(self.src.node_err(
                    child,
                    "this `folder` rule changes nothing; set bodies=, enabled= or idle=",
                ));
            }
            folders.push(rule);
        }
        Ok(SyncPolicy { bodies, folders })
    }

    fn folder_match(&self, node: &KdlNode) -> Result<FolderMatch, ConfigError> {
        let path = prop_str(self.src, node, "path")?;
        let role = prop_str(self.src, node, "role")?;
        match (path, role) {
            (Some((p, entry)), None) => {
                if p.is_empty() {
                    return Err(self.src.entry_err(entry, "`path` must not be empty"));
                }
                Ok(FolderMatch::Path(p.to_owned()))
            }
            (None, Some((r, entry))) => parse_role(r).map(FolderMatch::Role).ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    format!(
                        "`role` must be inbox, sent, drafts, archive, junk or trash, found {r:?}"
                    ),
                )
            }),
            (Some(_), Some((_, entry))) => Err(self
                .src
                .entry_err(entry, "use either `path` or `role`, not both")),
            (None, None) => Err(self
                .src
                .node_err(node, "`folder` needs path=\"…\" or role=\"…\"")),
        }
    }

    // --- virtual accounts ------------------------------------------------

    fn virtual_account(
        &self,
        node: &KdlNode,
        config: &AccountsConfig,
        identity_owner: &HashMap<IdentityId, AccountId>,
    ) -> Result<VirtualAccountConfig, ConfigError> {
        self.check_props(node, &["name", "default-identity", "merge-roles"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let id = self.id_arg(node, "the virtual account name")?;
        let display = self
            .prop_nonempty(node, "name")?
            .unwrap_or_else(|| id.to_string());
        let merge_roles = self.prop_bool(node, "merge-roles")?.unwrap_or(true);

        let mut members: Option<Vec<AccountId>> = None;
        let mut mappings: Vec<FolderMapping> = Vec::new();
        let mut rules: Vec<(IdentityRule, &KdlNode)> = Vec::new();
        for child in children(node) {
            match name(child) {
                "members" => {
                    if members.is_some() {
                        return Err(self.src.node_err(child, "`members` is set twice"));
                    }
                    self.check_props(child, &[])?;
                    check_no_children(self.src, child)?;
                    let mut list: Vec<AccountId> = Vec::new();
                    for entry in args(child) {
                        let member =
                            crate::kdl_util::entry_str(self.src, entry, "a member account")?;
                        let member = AccountId::new(member);
                        if config.account(&member).is_none() {
                            let hint = if config.virtual_account(&member).is_some() {
                                " (virtual accounts cannot be members)"
                            } else {
                                ""
                            };
                            return Err(self.src.entry_err(
                                entry,
                                format!("unknown account {:?}{hint}", member.as_str()),
                            ));
                        }
                        if list.contains(&member) {
                            return Err(self.src.entry_err(
                                entry,
                                format!("account {:?} is listed twice", member.as_str()),
                            ));
                        }
                        list.push(member);
                    }
                    if list.is_empty() {
                        return Err(self
                            .src
                            .node_err(child, "`members` needs at least one account"));
                    }
                    members = Some(list);
                }
                "map" => mappings.push(self.mapping(child)?),
                "identity-rule" => {
                    self.check_props(child, &["identity"])?;
                    check_max_args(self.src, child, 1)?;
                    check_no_children(self.src, child)?;
                    let (pattern, entry) = arg_str(self.src, child, 0, "an address pattern")?;
                    if !is_valid_identity_pattern(pattern) {
                        return Err(self.src.entry_err(
                            entry,
                            format!(
                                "invalid pattern {pattern:?} (use \"*@example.com\", \"*@*.example.com\" or a full address)"
                            ),
                        ));
                    }
                    let Some((identity, _)) = prop_str(self.src, child, "identity")? else {
                        return Err(self
                            .src
                            .node_err(child, "`identity-rule` needs identity=\"…\""));
                    };
                    rules.push((
                        IdentityRule {
                            pattern: pattern.to_owned(),
                            identity: IdentityId::new(identity),
                        },
                        child,
                    ));
                }
                other => {
                    return Err(self.src.node_err(
                        child,
                        format!(
                            "unknown node `{other}` in `virtual-account` (expected members, map or identity-rule)"
                        ),
                    ));
                }
            }
        }
        let members = members.ok_or_else(|| {
            self.src.node_err(
                node,
                format!(
                    "virtual account {:?} needs `members \"<account>\" …`",
                    id.as_str()
                ),
            )
        })?;

        let mut names = HashSet::new();
        for (mapping, child) in mappings
            .iter()
            .zip(children(node).iter().filter(|c| name(c) == "map"))
        {
            if !names.insert(mapping.name.clone()) {
                return Err(self
                    .src
                    .node_err(child, format!("folder {:?} is mapped twice", mapping.name)));
            }
            for (source, folder_node) in mapping.sources.iter().zip(children(child)) {
                if !members.contains(&source.account_id) {
                    return Err(self.src.node_err(
                        folder_node,
                        format!(
                            "account {:?} is not a member of this virtual account",
                            source.account_id.as_str()
                        ),
                    ));
                }
            }
        }

        let check_identity =
            |id: &IdentityId, at: &dyn Fn(String) -> ConfigError| match identity_owner.get(id) {
                None => Err(at(format!("unknown identity {:?}", id.as_str()))),
                Some(owner) if !members.contains(owner) => Err(at(format!(
                    "identity {:?} belongs to account {:?}, which is not a member",
                    id.as_str(),
                    owner.as_str()
                ))),
                Some(_) => Ok(()),
            };
        let default_identity = match prop_str(self.src, node, "default-identity")? {
            Some((text, entry)) => {
                let id = IdentityId::new(text);
                check_identity(&id, &|m| self.src.entry_err(entry, m))?;
                Some(id)
            }
            None => None,
        };
        for (rule, child) in &rules {
            let entry = child.entry("identity");
            check_identity(&rule.identity, &|m| match entry {
                Some(e) => self.src.entry_err(e, m),
                None => self.src.node_err(child, m),
            })?;
        }

        Ok(VirtualAccountConfig {
            id,
            name: display,
            members,
            merge_roles,
            folder_mappings: mappings,
            default_identity,
            identity_rules: rules.into_iter().map(|(r, _)| r).collect(),
        })
    }

    fn mapping(&self, node: &KdlNode) -> Result<FolderMapping, ConfigError> {
        self.check_props(node, &[])?;
        check_max_args(self.src, node, 1)?;
        let (folder_name, entry) = arg_str(self.src, node, 0, "the folder name")?;
        if folder_name.trim().is_empty() {
            return Err(self
                .src
                .entry_err(entry, "the folder name must not be empty"));
        }
        let mut sources = Vec::new();
        for child in children(node) {
            if name(child) != "folder" {
                return Err(self.src.node_err(
                    child,
                    format!("unknown node `{}` in `map` (expected folder)", name(child)),
                ));
            }
            self.check_props(child, &["account", "path"])?;
            check_max_args(self.src, child, 0)?;
            check_no_children(self.src, child)?;
            let Some((account, _)) = prop_str(self.src, child, "account")? else {
                return Err(self.src.node_err(child, "`folder` needs account=\"…\""));
            };
            let path = match prop_str(self.src, child, "path")? {
                Some(("", entry)) => {
                    return Err(self.src.entry_err(entry, "`path` must not be empty"));
                }
                Some((p, _)) => p.to_owned(),
                None => return Err(self.src.node_err(child, "`folder` needs path=\"…\"")),
            };
            let source = FolderSource {
                account_id: AccountId::new(account),
                path,
            };
            if sources.contains(&source) {
                return Err(self.src.node_err(child, "this folder is listed twice"));
            }
            sources.push(source);
        }
        if sources.is_empty() {
            return Err(self.src.node_err(
                node,
                "`map` needs at least one `folder account=\"…\" path=\"…\"`",
            ));
        }
        Ok(FolderMapping {
            name: folder_name.to_owned(),
            sources,
        })
    }

    // --- top-level settings ----------------------------------------------

    fn oauth_provider(&self, node: &KdlNode) -> Result<OAuthProviderConfig, ConfigError> {
        self.check_props(
            node,
            &["client-id", "client-secret", "auth-url", "token-url"],
        )?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let (id, entry) = arg_str(self.src, node, 0, "the provider id")?;
        if !crate::kdl_util::is_kebab(id) {
            return Err(self.src.entry_err(
                entry,
                format!("invalid provider id {id:?}: use lowercase letters, digits and `-`"),
            ));
        }
        let client_id = self.prop_nonempty(node, "client-id")?.ok_or_else(|| {
            self.src
                .node_err(node, format!("oauth-provider {id:?} needs client-id=\"…\""))
        })?;
        let auth_url = self.prop_url(node, "auth-url")?;
        let token_url = self.prop_url(node, "token-url")?;
        let built_in = OAuthProviderConfig::BUILT_IN.contains(&id);
        if !built_in && (auth_url.is_none() || token_url.is_none()) {
            return Err(self.src.node_err(
                node,
                format!("oauth-provider {id:?} is not built in and needs auth-url=\"…\" and token-url=\"…\""),
            ));
        }
        let mut scopes = Vec::new();
        for child in children(node) {
            if name(child) != "scopes" {
                return Err(self.src.node_err(
                    child,
                    format!(
                        "unknown node `{}` in `oauth-provider` (expected scopes)",
                        name(child)
                    ),
                ));
            }
            self.check_props(child, &[])?;
            check_no_children(self.src, child)?;
            for entry in args(child) {
                scopes.push(crate::kdl_util::entry_str(self.src, entry, "a scope")?.to_owned());
            }
        }
        Ok(OAuthProviderConfig {
            id: id.to_owned(),
            client_id,
            client_secret: self.prop_secret(node, "client-secret")?,
            auth_url,
            token_url,
            scopes,
        })
    }

    fn undo_send_delay(&self, node: &KdlNode) -> Result<u32, ConfigError> {
        self.check_props(node, &[])?;
        check_no_children(self.src, node)?;
        check_max_args(self.src, node, 1)?;
        let entry = *args(node).first().ok_or_else(|| {
            self.src
                .node_err(node, "`undo-send-delay` needs a number of seconds")
        })?;
        entry
            .value()
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .filter(|v| *v <= MAX_UNDO_SEND_SECS)
            .ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    format!(
                        "`undo-send-delay` must be a whole number of seconds from 0 to {MAX_UNDO_SEND_SECS}, found {}",
                        describe(entry.value())
                    ),
                )
            })
    }
}

fn parse_role(text: &str) -> Option<FolderRole> {
    Some(match text {
        "inbox" => FolderRole::Inbox,
        "sent" => FolderRole::Sent,
        "drafts" => FolderRole::Drafts,
        "archive" => FolderRole::Archive,
        "junk" => FolderRole::Junk,
        "trash" => FolderRole::Trash,
        _ => return None,
    })
}

pub(crate) fn role_name(role: FolderRole) -> &'static str {
    match role {
        FolderRole::Inbox => "inbox",
        FolderRole::Sent => "sent",
        FolderRole::Drafts => "drafts",
        FolderRole::Archive => "archive",
        FolderRole::Junk => "junk",
        FolderRole::Trash => "trash",
        FolderRole::Other => "other",
    }
}

fn parse_mechanism(text: &str) -> Option<SaslMechanism> {
    Some(match text {
        "plain" => SaslMechanism::Plain,
        "login" => SaslMechanism::Login,
        "scram-sha-256" => SaslMechanism::ScramSha256,
        "apop" => SaslMechanism::Apop,
        "xoauth2" => SaslMechanism::Xoauth2,
        "oauthbearer" => SaslMechanism::Oauthbearer,
        "external" => SaslMechanism::External,
        _ => return None,
    })
}

pub(crate) fn mechanism_name(mechanism: SaslMechanism) -> &'static str {
    match mechanism {
        SaslMechanism::Plain => "plain",
        SaslMechanism::Login => "login",
        SaslMechanism::ScramSha256 => "scram-sha-256",
        SaslMechanism::Apop => "apop",
        SaslMechanism::Xoauth2 => "xoauth2",
        SaslMechanism::Oauthbearer => "oauthbearer",
        SaslMechanism::External => "external",
    }
}

/// A light plausibility check: `local@domain` with a dot-free or dotted
/// domain, no whitespace. Real validation happens at the server.
pub(crate) fn is_plausible_email(text: &str) -> bool {
    let Some((local, domain)) = text.rsplit_once('@') else {
        return false;
    };
    !local.is_empty()
        && !domain.is_empty()
        && !domain.starts_with('.')
        && !domain.ends_with('.')
        && !text
            .chars()
            .any(|c| c.is_whitespace() || c.is_control() || c == '<' || c == '>')
}

/// `https://…`, or `http://` for loopback hosts only (mock servers in tests).
fn is_allowed_url(text: &str) -> bool {
    if text.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    if let Some(rest) = text.strip_prefix("https://") {
        return !rest.is_empty() && !rest.starts_with('/');
    }
    if let Some(rest) = text.strip_prefix("http://") {
        let host = rest.split(['/', '?', '#']).next().unwrap_or_default();
        let host = host.rsplit_once(':').map_or(host, |(h, port)| {
            if port.chars().all(|c| c.is_ascii_digit()) {
                h
            } else {
                host
            }
        });
        return matches!(host, "localhost" | "127.0.0.1" | "[::1]");
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emails() {
        assert!(is_plausible_email("ada@example.org"));
        assert!(is_plausible_email("a+b@localhost"));
        assert!(!is_plausible_email("ada"));
        assert!(!is_plausible_email("@example.org"));
        assert!(!is_plausible_email("ada@"));
        assert!(!is_plausible_email("ada @example.org"));
        assert!(!is_plausible_email("ada@.org"));
    }

    #[test]
    fn urls() {
        assert!(is_allowed_url("https://accounts.example/o/auth"));
        assert!(is_allowed_url("http://127.0.0.1:8080/token"));
        assert!(is_allowed_url("http://localhost/token"));
        assert!(!is_allowed_url("http://example.org/token"));
        assert!(!is_allowed_url("ftp://example.org"));
        assert!(!is_allowed_url("https://"));
    }

    #[test]
    fn names_round_trip() {
        for m in [
            SaslMechanism::Plain,
            SaslMechanism::Login,
            SaslMechanism::ScramSha256,
            SaslMechanism::Apop,
            SaslMechanism::Xoauth2,
            SaslMechanism::Oauthbearer,
            SaslMechanism::External,
        ] {
            assert_eq!(parse_mechanism(mechanism_name(m)), Some(m));
        }
        for r in [
            FolderRole::Inbox,
            FolderRole::Sent,
            FolderRole::Drafts,
            FolderRole::Archive,
            FolderRole::Junk,
            FolderRole::Trash,
        ] {
            assert_eq!(parse_role(role_name(r)), Some(r));
        }
        assert_eq!(parse_role("other"), None);
    }
}
