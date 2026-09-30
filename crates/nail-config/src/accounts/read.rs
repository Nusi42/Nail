//! Reading `accounts.kdl` into [`AccountsConfig`], validating every node,
//! property and cross reference with precise positions.

use std::collections::{HashMap, HashSet};

use kdl::{KdlEntry, KdlNode, KdlValue};
use nail_api::accounts::{
    AccountConfig, AccountsConfig, AuthConfig, BodyPolicy, ComposerMode, FolderMapping,
    FolderMatch, FolderPolicy, FolderSource, Identity, IdentityRule, ImapOptions, IncomingServer,
    MailProtocol, MailStyleConfig, OAuthProviderConfig, OutgoingServer, Pop3Options, QuotePosting,
    SaslMechanism, SecretRef, ServerEndpoint, Signature, SmtpOptions, SyncPolicy, TlsMode,
    VirtualAccountConfig, default_port,
};
use nail_api::security::{AutocryptPreference, CryptoPreference, IdentityCrypto};
use nail_api::sieve::{ManageSieveConfig, is_script_name};
use nail_api::{AccountId, FolderRole, IdentityId, LocalizedMessage};

use super::{
    MAX_UNDO_SEND_SECS, MIN_POLL_INTERVAL_SECS, is_valid_account_id, is_valid_identity_pattern,
};
use crate::error::ConfigError;
use crate::kdl_util::{
    Errors, Source, arg_str, args, check_max_args, check_no_children, check_props, children,
    describe, name, prop_str, props, what_arg, what_key,
};
use crate::l10n::msg;

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
    read_all(src).map_err(|mut errors| errors.swap_remove(0))
}

/// Reads the file, reporting every error: each top-level node (account,
/// virtual account, OAuth provider, …) is checked on its own, so one bad
/// node does not hide the errors of the others.
pub(crate) fn read_all(src: &Source<'_>) -> Result<AccountsConfig, Vec<ConfigError>> {
    let doc = src.parse_all()?;
    let errors = Errors::default();
    let reader = Reader { src };
    let mut config = AccountsConfig::default();
    let mut ids: HashSet<String> = HashSet::new();
    let mut identity_ids: HashMap<IdentityId, AccountId> = HashMap::new();
    let mut provider_ids: HashSet<String> = HashSet::new();
    let mut undo_seen = false;
    let mut virtual_nodes = Vec::new();
    let mut oauth_refs: Vec<(String, &KdlNode)> = Vec::new();

    for node in doc.nodes() {
        let result = (|| -> Result<(), ConfigError> {
            match name(node) {
                "account" => {
                    let (account, provider_refs) = reader.account(node)?;
                    if !ids.insert(account.id.to_string()) {
                        return Err(src.node_err(
                            node,
                            msg("config-account-duplicate").arg("name", account.id.as_str()),
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
                                msg("config-identity-duplicate").arg("id", identity.id.as_str()),
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
                            msg("config-oauth-provider-duplicate").arg("id", provider.id.as_str()),
                        ));
                    }
                    config.oauth_providers.push(provider);
                }
                "undo-send-delay" => {
                    if undo_seen {
                        return Err(src.node_err(
                            node,
                            msg("config-set-twice").arg("node", "undo-send-delay"),
                        ));
                    }
                    undo_seen = true;
                    config.undo_send_secs = Some(reader.undo_send_delay(node)?);
                }
                other => {
                    return Err(src.node_err(
                        node,
                        msg("config-unknown-node").arg("node", other).arg(
                            "expected",
                            "account, virtual-account, oauth-provider, undo-send-delay",
                        ),
                    ));
                }
            }
            Ok(())
        })();
        errors.ok(result);
    }

    for (provider, node) in oauth_refs {
        let built_in = OAuthProviderConfig::BUILT_IN.contains(&provider.as_str());
        if !built_in && !provider_ids.contains(&provider) {
            errors.push(src.node_err(
                node,
                msg("config-oauth-provider-unknown").arg("id", provider.as_str()),
            ));
        }
    }

    for node in virtual_nodes {
        let Some(virtual_account) = errors.ok(reader.virtual_account(node, &config, &identity_ids))
        else {
            continue;
        };
        if !ids.insert(virtual_account.id.to_string()) {
            errors.push(src.node_err(
                node,
                msg("config-account-name-taken").arg("name", virtual_account.id.as_str()),
            ));
        }
        config.virtual_accounts.push(virtual_account);
    }
    // Virtual accounts keep their file order relative to each other; they
    // are read after the real accounts only to resolve references.
    errors.finish(config)
}

struct Reader<'s, 'a> {
    src: &'s Source<'a>,
}

impl Reader<'_, '_> {
    fn check_props(&self, node: &KdlNode, allowed: &[&str]) -> Result<(), ConfigError> {
        for (key, entry) in props(node) {
            if SECRET_PROPS.contains(&key) {
                return Err(self
                    .src
                    .entry_err(entry, msg("config-secret-not-allowed").arg("key", key)));
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
                    .entry_err(entry, msg("config-duplicate-property").arg("key", key)));
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
                .entry_err(entry, msg("config-empty").arg("what", what_key(key)))),
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
                    msg("config-expected-bool")
                        .arg("what", what_key(key))
                        .arg("found", describe(other)),
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
                msg("config-expected-range")
                    .arg("what", what_key(key))
                    .arg("min", min)
                    .arg("max", max)
                    .arg("found", describe(entry.value())),
            )
        })
    }

    fn prop_secret(&self, node: &KdlNode, key: &str) -> Result<Option<SecretRef>, ConfigError> {
        match prop_str(self.src, node, key)? {
            None => Ok(None),
            Some((text, entry)) => SecretRef::parse(text).map(Some).map_err(|_| {
                self.src.entry_err(
                    entry,
                    msg("config-secret-invalid")
                        .arg("key", key)
                        .arg("found", format!("{text:?}")),
                )
            }),
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
                        msg("config-expected-url")
                            .arg("what", what_key(key))
                            .arg("found", format!("{text:?}")),
                    ))
                }
            }
        }
    }

    fn id_arg(&self, node: &KdlNode) -> Result<AccountId, ConfigError> {
        let (id, entry) = arg_str(self.src, node, 0)?;
        if id == AccountId::ALL {
            return Err(self.src.entry_err(
                entry,
                msg("config-account-name-reserved").arg("name", AccountId::ALL),
            ));
        }
        if !is_valid_account_id(id) {
            return Err(self
                .src
                .entry_err(entry, msg("config-account-name-invalid").arg("name", id)));
        }
        Ok(AccountId::new(id))
    }

    fn email(&self, entry: &KdlEntry, text: &str, what: &str) -> Result<String, ConfigError> {
        if is_plausible_email(text) {
            Ok(text.to_owned())
        } else {
            Err(self.src.entry_err(
                entry,
                msg("config-expected-email")
                    .arg("what", what)
                    .arg("found", format!("{text:?}")),
            ))
        }
    }

    // --- account ---------------------------------------------------------

    /// Reads an `account` node; also returns the OAuth provider ids it uses.
    fn account(&self, node: &KdlNode) -> Result<(AccountConfig, Vec<String>), ConfigError> {
        self.check_props(node, &["name", "email", "enabled", "save-sent-copy"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let id = self.id_arg(node)?;
        let email = match prop_str(self.src, node, "email")? {
            Some((text, entry)) => self.email(entry, text, "`email`")?,
            None => {
                return Err(self.src.node_err(
                    node,
                    msg("config-account-needs-email").arg("name", id.as_str()),
                ));
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
        let mut authserv_ids: Option<Vec<String>> = None;
        let mut sieve: Option<(ManageSieveConfig, &KdlNode)> = None;
        let mut providers = Vec::new();
        for child in children(node) {
            match name(child) {
                "authserv-id" => {
                    if authserv_ids.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "authserv-id")));
                    }
                    authserv_ids = Some(self.authserv_ids(child)?);
                }
                protocol @ ("imap" | "pop3") => {
                    if incoming.is_some() {
                        return Err(self.src.node_err(child, msg("config-account-one-incoming")));
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
                        return Err(self.src.node_err(child, msg("config-account-one-smtp")));
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
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "sync")));
                    }
                    sync = Some(self.sync(child)?);
                }
                "sieve" => {
                    if sieve.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "sieve")));
                    }
                    sieve = Some((self.sieve(child)?, child));
                }
                other => {
                    return Err(self.src.node_err(
                        child,
                        msg("config-unknown-child")
                            .arg("node", other)
                            .arg("parent", "account")
                            .arg(
                                "expected",
                                "imap, pop3, smtp, identity, sync, sieve, authserv-id",
                            ),
                    ));
                }
            }
        }
        let incoming = incoming.ok_or_else(|| {
            self.src.node_err(
                node,
                msg("config-account-needs-incoming").arg("name", id.as_str()),
            )
        })?;
        if let Some((_, child)) = &sieve
            && !matches!(incoming, IncomingServer::Imap { .. })
        {
            return Err(self.src.node_err(child, msg("config-sieve-needs-imap")));
        }
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
                posting: None,
                mail_style: None,
                crypto: None,
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
                    msg("config-identity-duplicate").arg("id", identity.id.as_str()),
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
                authserv_ids: authserv_ids.unwrap_or_default(),
                sieve: sieve.map(|(config, _)| config),
            },
            providers,
        ))
    }

    /// Reads `sieve host="…" port=4190 tls="starttls" script="nail"
    /// enabled=#true` (ManageSieve, M2c); every property is optional.
    fn sieve(&self, node: &KdlNode) -> Result<ManageSieveConfig, ConfigError> {
        self.check_props(node, &["enabled", "host", "port", "tls", "script"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 0)?;
        check_no_children(self.src, node)?;
        let host = match prop_str(self.src, node, "host")? {
            None => None,
            Some((h, entry)) => {
                if h.is_empty()
                    || h.chars().any(|c| c.is_whitespace() || c.is_control())
                    || h.contains('/')
                {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-expected-host")
                            .arg("what", "`host`")
                            .arg("found", format!("{h:?}")),
                    ));
                }
                Some(h.to_owned())
            }
        };
        let tls = match prop_str(self.src, node, "tls")? {
            None => None,
            Some(("implicit", _)) => Some(TlsMode::Implicit),
            Some(("starttls", _)) => Some(TlsMode::Starttls),
            Some(("plaintext", _)) => Some(TlsMode::Plaintext),
            Some((t, entry)) => {
                return Err(self.src.entry_err(
                    entry,
                    msg("config-expected-one-of")
                        .arg("what", "`tls`")
                        .arg("choices", "\"implicit\", \"starttls\", \"plaintext\"")
                        .arg("found", format!("{t:?}")),
                ));
            }
        };
        let port = self
            .prop_u32_range(node, "port", 1, 65535)?
            .map(|p| u16::try_from(p).unwrap_or(u16::MAX));
        let script_name = match prop_str(self.src, node, "script")? {
            None => None,
            Some((s, entry)) => {
                if !is_script_name(s) {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-sieve-script-invalid").arg("found", format!("{s:?}")),
                    ));
                }
                Some(s.to_owned())
            }
        };
        Ok(ManageSieveConfig {
            enabled: self.prop_bool(node, "enabled")?,
            host,
            port,
            tls,
            script_name,
        })
    }

    /// Reads `authserv-id "mx.example.net" …`: the `Authentication-Results`
    /// server ids to trust (lowercased, no duplicates).
    fn authserv_ids(&self, node: &KdlNode) -> Result<Vec<String>, ConfigError> {
        self.check_props(node, &[])?;
        check_no_children(self.src, node)?;
        let list = args(node);
        if list.is_empty() {
            return Err(self.src.node_err(node, msg("config-authserv-id-empty")));
        }
        let mut ids = Vec::new();
        for entry in list {
            let text = crate::kdl_util::entry_str(self.src, entry, "`authserv-id`")?.trim();
            if text.is_empty() || text.chars().any(|c| c.is_whitespace() || c == ';') {
                return Err(self.src.entry_err(
                    entry,
                    msg("config-authserv-id-invalid").arg("found", format!("{text:?}")),
                ));
            }
            let id = text.to_ascii_lowercase();
            if !ids.contains(&id) {
                ids.push(id);
            }
        }
        Ok(ids)
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
                        msg("config-expected-host")
                            .arg("what", "`host`")
                            .arg("found", format!("{h:?}")),
                    ));
                }
                h.to_owned()
            }
            None => {
                return Err(self.src.node_err(
                    node,
                    msg("config-needs-property")
                        .arg("node", name(node))
                        .arg("key", "host"),
                ));
            }
        };
        let tls = match prop_str(self.src, node, "tls")? {
            None => TlsMode::Implicit,
            Some((t, entry)) => match t {
                "implicit" => TlsMode::Implicit,
                "starttls" => TlsMode::Starttls,
                "plaintext" => TlsMode::Plaintext,
                _ => {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-expected-one-of")
                            .arg("what", "`tls`")
                            .arg("choices", "\"implicit\", \"starttls\", \"plaintext\"")
                            .arg("found", format!("{t:?}")),
                    ));
                }
            },
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
                    msg("config-unknown-child")
                        .arg("node", name(child))
                        .arg("parent", name(node))
                        .arg("expected", "auth"),
                ));
            }
            if auth.is_some() {
                return Err(self
                    .src
                    .node_err(child, msg("config-set-twice").arg("node", "auth")));
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
        let (method, method_entry) = arg_str(self.src, node, 0)?;
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
                    msg("config-expected-one-of")
                        .arg("what", "`mechanism`")
                        .arg("choices", names.join(", "))
                        .arg("found", format!("{text:?}")),
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
                        return Err(self.src.node_err(node, msg("config-oauth-needs-provider")));
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
                    msg("config-auth-method-unknown").arg("found", format!("{other:?}")),
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
                    return Err(self
                        .src
                        .entry_err(entry, msg("config-needs-leave-on-server").arg("key", key)));
                }
            }
        }
        Ok(options)
    }

    fn identity(&self, node: &KdlNode, account: &AccountId) -> Result<Identity, ConfigError> {
        self.check_props(node, &["id", "name", "reply-to", "composer", "posting"])?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 1)?;
        let (email, entry) = arg_str(self.src, node, 0)?;
        let email = self.email(entry, email, "`identity`")?;
        let id = match prop_str(self.src, node, "id")? {
            Some((id, entry)) => {
                if id.is_empty() || id.chars().any(|c| c.is_whitespace() || c.is_control()) {
                    return Err(self.src.entry_err(entry, msg("config-identity-id-invalid")));
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
            Some((text, entry)) => Some(match text {
                "plain" => ComposerMode::Plain,
                "html" => ComposerMode::Html,
                "markdown" => ComposerMode::Markdown,
                _ => {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-expected-one-of")
                            .arg("what", "`composer`")
                            .arg("choices", "\"plain\", \"html\", \"markdown\"")
                            .arg("found", format!("{text:?}")),
                    ));
                }
            }),
        };
        let posting = match prop_str(self.src, node, "posting")? {
            None => None,
            Some(("top", _)) => Some(QuotePosting::Top),
            Some(("bottom", _)) => Some(QuotePosting::Bottom),
            Some((text, entry)) => {
                return Err(self.src.entry_err(
                    entry,
                    msg("config-expected-one-of")
                        .arg("what", "`posting`")
                        .arg("choices", "\"top\", \"bottom\"")
                        .arg("found", format!("{text:?}")),
                ));
            }
        };
        let mut bcc = Vec::new();
        let mut signature: Option<Signature> = None;
        let mut mail_style: Option<MailStyleConfig> = None;
        let mut crypto: Option<IdentityCrypto> = None;
        for child in children(node) {
            match name(child) {
                "bcc" => {
                    self.check_props(child, &[])?;
                    check_no_children(self.src, child)?;
                    let list = args(child);
                    if list.is_empty() {
                        return Err(self.src.node_err(child, msg("config-bcc-empty")));
                    }
                    for entry in list {
                        let text = crate::kdl_util::entry_str(self.src, entry, "`bcc`")?;
                        bcc.push(self.email(entry, text, "`bcc`")?);
                    }
                }
                "signature" => {
                    if signature.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "signature")));
                    }
                    self.check_props(child, &["html", "markdown"])?;
                    self.check_unique_props(child)?;
                    check_max_args(self.src, child, 1)?;
                    check_no_children(self.src, child)?;
                    let text = match args(child).first() {
                        Some(entry) => Some(
                            crate::kdl_util::entry_str(self.src, entry, "`signature`")?.to_owned(),
                        ),
                        None => None,
                    };
                    signature = Some(Signature {
                        text,
                        html: self.prop_string(child, "html")?,
                        markdown: self.prop_string(child, "markdown")?,
                    });
                }
                "mail-style" => {
                    if mail_style.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "mail-style")));
                    }
                    self.check_props(child, &["css", "code-theme"])?;
                    self.check_unique_props(child)?;
                    check_max_args(self.src, child, 0)?;
                    check_no_children(self.src, child)?;
                    mail_style = Some(MailStyleConfig {
                        css: self.prop_string(child, "css")?,
                        code_theme: self.prop_nonempty(child, "code-theme")?,
                    });
                }
                "crypto" => {
                    if crypto.is_some() {
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "crypto")));
                    }
                    crypto = Some(self.identity_crypto(child)?);
                }
                other => {
                    return Err(self.src.node_err(
                        child,
                        msg("config-unknown-child")
                            .arg("node", other)
                            .arg("parent", "identity")
                            .arg("expected", "bcc, signature, mail-style, crypto"),
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
            posting,
            mail_style,
            crypto,
        })
    }

    /// `crypto technology=… sign=… encrypt=… openpgp-key=… smime-cert=…
    /// autocrypt=… prefer-encrypt=… protect-headers=…` (M2a, ADR 0029).
    fn identity_crypto(&self, node: &KdlNode) -> Result<IdentityCrypto, ConfigError> {
        self.check_props(
            node,
            &[
                "technology",
                "sign",
                "encrypt",
                "openpgp-key",
                "smime-cert",
                "autocrypt",
                "prefer-encrypt",
                "protect-headers",
            ],
        )?;
        self.check_unique_props(node)?;
        check_max_args(self.src, node, 0)?;
        check_no_children(self.src, node)?;
        let defaults = IdentityCrypto::default();
        let technology = match prop_str(self.src, node, "technology")? {
            None => defaults.technology,
            Some(("auto", _)) => CryptoPreference::Auto,
            Some(("openpgp", _)) => CryptoPreference::OpenPgp,
            Some(("smime", _)) => CryptoPreference::Smime,
            Some((text, entry)) => {
                return Err(self.src.entry_err(
                    entry,
                    msg("config-expected-one-of")
                        .arg("what", "`technology`")
                        .arg("choices", "\"auto\", \"openpgp\", \"smime\"")
                        .arg("found", format!("{text:?}")),
                ));
            }
        };
        let prefer_encrypt = match prop_str(self.src, node, "prefer-encrypt")? {
            None => defaults.prefer_encrypt,
            Some(("mutual", _)) => AutocryptPreference::Mutual,
            Some(("nopreference", _)) => AutocryptPreference::NoPreference,
            Some((text, entry)) => {
                return Err(self.src.entry_err(
                    entry,
                    msg("config-expected-one-of")
                        .arg("what", "`prefer-encrypt`")
                        .arg("choices", "\"mutual\", \"nopreference\"")
                        .arg("found", format!("{text:?}")),
                ));
            }
        };
        Ok(IdentityCrypto {
            technology,
            sign: self.prop_bool(node, "sign")?.unwrap_or(defaults.sign),
            encrypt: self.prop_bool(node, "encrypt")?.unwrap_or(defaults.encrypt),
            openpgp_key: self.fingerprint(node, "openpgp-key", &[40, 64])?,
            smime_certificate: self.fingerprint(node, "smime-cert", &[64])?,
            autocrypt: self
                .prop_bool(node, "autocrypt")?
                .unwrap_or(defaults.autocrypt),
            prefer_encrypt,
            protect_headers: self
                .prop_bool(node, "protect-headers")?
                .unwrap_or(defaults.protect_headers),
        })
    }

    /// A hexadecimal fingerprint property (spaces allowed, stored
    /// uppercase without them) of one of the given lengths.
    fn fingerprint(
        &self,
        node: &KdlNode,
        key: &str,
        lengths: &[usize],
    ) -> Result<Option<String>, ConfigError> {
        let Some((text, entry)) = prop_str(self.src, node, key)? else {
            return Ok(None);
        };
        let hex: String = text.chars().filter(|c| *c != ' ').collect();
        if !hex.chars().all(|c| c.is_ascii_hexdigit()) || !lengths.contains(&hex.len()) {
            let lengths = lengths
                .iter()
                .map(ToString::to_string)
                .collect::<Vec<_>>()
                .join(" or ");
            return Err(self.src.entry_err(
                entry,
                msg("config-fingerprint-invalid")
                    .arg("key", key)
                    .arg("lengths", lengths)
                    .arg("found", format!("{text:?}")),
            ));
        }
        Ok(Some(hex.to_ascii_uppercase()))
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
                            .entry_err(entry, msg("config-sync-recent-needs-days"))
                    })?,
                },
                _ => {
                    return Err(self.src.entry_err(
                        entry,
                        msg("config-expected-one-of")
                            .arg("what", "`bodies`")
                            .arg("choices", "\"full\", \"headers-only\", \"recent\"")
                            .arg("found", format!("{text:?}")),
                    ));
                }
            }),
        };
        if !matches!(policy, Some(BodyPolicy::Recent { .. }))
            && let Some(entry) = node.entry("days")
        {
            return Err(self
                .src
                .entry_err(entry, msg("config-sync-days-needs-recent")));
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
                    msg("config-unknown-child")
                        .arg("node", name(child))
                        .arg("parent", "sync")
                        .arg("expected", "folder"),
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
                return Err(self.src.node_err(child, msg("config-sync-rule-empty")));
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
                    return Err(self
                        .src
                        .entry_err(entry, msg("config-empty").arg("what", "`path`")));
                }
                Ok(FolderMatch::Path(p.to_owned()))
            }
            (None, Some((r, entry))) => parse_role(r).map(FolderMatch::Role).ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    msg("config-expected-one-of")
                        .arg("what", "`role`")
                        .arg("choices", "inbox, sent, drafts, archive, junk, trash")
                        .arg("found", format!("{r:?}")),
                )
            }),
            (Some(_), Some((_, entry))) => {
                Err(self.src.entry_err(entry, msg("config-path-or-role")))
            }
            (None, None) => Err(self
                .src
                .node_err(node, msg("config-folder-needs-path-or-role"))),
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
        let id = self.id_arg(node)?;
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
                        return Err(self
                            .src
                            .node_err(child, msg("config-set-twice").arg("node", "members")));
                    }
                    self.check_props(child, &[])?;
                    check_no_children(self.src, child)?;
                    let mut list: Vec<AccountId> = Vec::new();
                    for entry in args(child) {
                        let member = crate::kdl_util::entry_str(self.src, entry, "`members`")?;
                        let member = AccountId::new(member);
                        if config.account(&member).is_none() {
                            return Err(self.src.entry_err(
                                entry,
                                msg(if config.virtual_account(&member).is_some() {
                                    "config-member-virtual"
                                } else {
                                    "config-member-unknown"
                                })
                                .arg("name", member.as_str()),
                            ));
                        }
                        if list.contains(&member) {
                            return Err(self.src.entry_err(
                                entry,
                                msg("config-member-duplicate").arg("name", member.as_str()),
                            ));
                        }
                        list.push(member);
                    }
                    if list.is_empty() {
                        return Err(self.src.node_err(child, msg("config-members-empty")));
                    }
                    members = Some(list);
                }
                "map" => mappings.push(self.mapping(child)?),
                "identity-rule" => {
                    self.check_props(child, &["identity"])?;
                    check_max_args(self.src, child, 1)?;
                    check_no_children(self.src, child)?;
                    let (pattern, entry) = arg_str(self.src, child, 0)?;
                    if !is_valid_identity_pattern(pattern) {
                        return Err(self.src.entry_err(
                            entry,
                            msg("config-identity-pattern-invalid")
                                .arg("found", format!("{pattern:?}")),
                        ));
                    }
                    let Some((identity, _)) = prop_str(self.src, child, "identity")? else {
                        return Err(self.src.node_err(
                            child,
                            msg("config-needs-property")
                                .arg("node", "identity-rule")
                                .arg("key", "identity"),
                        ));
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
                        msg("config-unknown-child")
                            .arg("node", other)
                            .arg("parent", "virtual-account")
                            .arg("expected", "members, map, identity-rule"),
                    ));
                }
            }
        }
        let members = members.ok_or_else(|| {
            self.src.node_err(
                node,
                msg("config-virtual-needs-members").arg("name", id.as_str()),
            )
        })?;

        let mut names = HashSet::new();
        for (mapping, child) in mappings
            .iter()
            .zip(children(node).iter().filter(|c| name(c) == "map"))
        {
            if !names.insert(mapping.name.clone()) {
                return Err(self.src.node_err(
                    child,
                    msg("config-map-folder-duplicate").arg("name", mapping.name.as_str()),
                ));
            }
            for (source, folder_node) in mapping.sources.iter().zip(children(child)) {
                if !members.contains(&source.account_id) {
                    return Err(self.src.node_err(
                        folder_node,
                        msg("config-map-not-member").arg("name", source.account_id.as_str()),
                    ));
                }
            }
        }

        let check_identity =
            |id: &IdentityId, at: &dyn Fn(LocalizedMessage) -> ConfigError| match identity_owner
                .get(id)
            {
                None => Err(at(msg("config-identity-unknown").arg("id", id.as_str()))),
                Some(owner) if !members.contains(owner) => {
                    Err(at(msg("config-identity-not-member")
                        .arg("id", id.as_str())
                        .arg("account", owner.as_str())))
                }
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
        let (folder_name, entry) = arg_str(self.src, node, 0)?;
        if folder_name.trim().is_empty() {
            return Err(self
                .src
                .entry_err(entry, msg("config-empty").arg("what", what_arg(node, 0))));
        }
        let mut sources = Vec::new();
        for child in children(node) {
            if name(child) != "folder" {
                return Err(self.src.node_err(
                    child,
                    msg("config-unknown-child")
                        .arg("node", name(child))
                        .arg("parent", "map")
                        .arg("expected", "folder"),
                ));
            }
            self.check_props(child, &["account", "path"])?;
            check_max_args(self.src, child, 0)?;
            check_no_children(self.src, child)?;
            let Some((account, _)) = prop_str(self.src, child, "account")? else {
                return Err(self.src.node_err(
                    child,
                    msg("config-needs-property")
                        .arg("node", "folder")
                        .arg("key", "account"),
                ));
            };
            let path = match prop_str(self.src, child, "path")? {
                Some(("", entry)) => {
                    return Err(self
                        .src
                        .entry_err(entry, msg("config-empty").arg("what", "`path`")));
                }
                Some((p, _)) => p.to_owned(),
                None => {
                    return Err(self.src.node_err(
                        child,
                        msg("config-needs-property")
                            .arg("node", "folder")
                            .arg("key", "path"),
                    ));
                }
            };
            let source = FolderSource {
                account_id: AccountId::new(account),
                path,
            };
            if sources.contains(&source) {
                return Err(self.src.node_err(child, msg("config-map-entry-duplicate")));
            }
            sources.push(source);
        }
        if sources.is_empty() {
            return Err(self.src.node_err(node, msg("config-map-empty")));
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
        let (id, entry) = arg_str(self.src, node, 0)?;
        if !crate::kdl_util::is_kebab(id) {
            return Err(self
                .src
                .entry_err(entry, msg("config-oauth-provider-id-invalid").arg("id", id)));
        }
        let client_id = self.prop_nonempty(node, "client-id")?.ok_or_else(|| {
            self.src.node_err(
                node,
                msg("config-oauth-provider-needs-client-id").arg("id", id),
            )
        })?;
        let auth_url = self.prop_url(node, "auth-url")?;
        let token_url = self.prop_url(node, "token-url")?;
        let built_in = OAuthProviderConfig::BUILT_IN.contains(&id);
        if !built_in && (auth_url.is_none() || token_url.is_none()) {
            return Err(self
                .src
                .node_err(node, msg("config-oauth-provider-needs-urls").arg("id", id)));
        }
        let mut scopes = Vec::new();
        for child in children(node) {
            if name(child) != "scopes" {
                return Err(self.src.node_err(
                    child,
                    msg("config-unknown-child")
                        .arg("node", name(child))
                        .arg("parent", "oauth-provider")
                        .arg("expected", "scopes"),
                ));
            }
            self.check_props(child, &[])?;
            check_no_children(self.src, child)?;
            for entry in args(child) {
                scopes.push(crate::kdl_util::entry_str(self.src, entry, "`scopes`")?.to_owned());
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
                .node_err(node, msg("config-undo-send-needs-seconds"))
        })?;
        entry
            .value()
            .as_integer()
            .and_then(|i| u32::try_from(i).ok())
            .filter(|v| *v <= MAX_UNDO_SEND_SECS)
            .ok_or_else(|| {
                self.src.entry_err(
                    entry,
                    msg("config-expected-range")
                        .arg("what", "`undo-send-delay`")
                        .arg("min", 0)
                        .arg("max", MAX_UNDO_SEND_SECS)
                        .arg("found", describe(entry.value())),
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
