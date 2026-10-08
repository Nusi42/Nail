//! Thunderbird accounts (`prefs.js`) as Nail account proposals. Passwords
//! are never read (Thunderbird keeps them in `logins.json`, encrypted);
//! every proposal says so with a `secret-not-imported` issue.

use nail_api::accounts::{
    AccountConfig, AuthConfig, Identity, ImapOptions, IncomingServer, MailProtocol, OutgoingServer,
    Pop3Options, ServerEndpoint, Signature, SmtpOptions, SyncPolicy, TlsMode, default_port,
};
use nail_api::ids::AccountId;
use nail_api::portability::{
    AccountProposal, IssueSeverity, ItemIssue, ItemRef, PortabilityIssueCode,
};

use super::prefs::Prefs;

/// Thunderbird's server types.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ServerType {
    /// IMAP.
    Imap,
    /// POP3.
    Pop3,
    /// Local Folders.
    None,
    /// Movemail (local mail spool).
    Movemail,
    /// RSS feeds.
    Rss,
    /// News (NNTP).
    Nntp,
    /// Anything else (Exchange add-ons, …).
    Other,
}

impl ServerType {
    fn parse(text: &str) -> Self {
        match text {
            "imap" => Self::Imap,
            "pop3" => Self::Pop3,
            "none" => Self::None,
            "movemail" => Self::Movemail,
            "rss" => Self::Rss,
            "nntp" => Self::Nntp,
            _ => Self::Other,
        }
    }

    /// Whether the server keeps its mail in local mbox files (Local
    /// Folders, POP3, movemail) — those are imported as mail.
    pub fn has_local_mail(self) -> bool {
        matches!(self, Self::None | Self::Pop3 | Self::Movemail)
    }
}

/// One Thunderbird account with its incoming server.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TbAccount {
    /// `account3`.
    pub key: String,
    /// `server2`.
    pub server_key: String,
    /// The server type.
    pub server_type: ServerType,
    /// The account's name (`mail.server.*.name`).
    pub name: String,
    /// The server host.
    pub host: Option<String>,
    /// The identities' keys.
    pub identities: Vec<String>,
}

/// The accounts in `mail.accountmanager.accounts` order.
pub fn accounts(prefs: &Prefs) -> Vec<TbAccount> {
    let mut keys = prefs.list("mail.accountmanager.accounts");
    if keys.is_empty() {
        // Old or partial profiles: every `mail.account.*.server`.
        keys = prefs
            .names_with_prefix("mail.account.")
            .into_iter()
            .filter_map(|n| n.strip_prefix("mail.account.")?.strip_suffix(".server"))
            .map(str::to_owned)
            .collect();
    }
    keys.into_iter()
        .filter_map(|key| {
            let server_key = prefs.text(&format!("mail.account.{key}.server"))?;
            let server = |name: &str| format!("mail.server.{server_key}.{name}");
            let server_type = ServerType::parse(&prefs.text(&server("type")).unwrap_or_default());
            let host = prefs
                .text(&server("hostname"))
                .or_else(|| prefs.text(&server("realhostname")));
            let name = prefs
                .text(&server("name"))
                .or_else(|| host.clone())
                .unwrap_or_else(|| key.clone());
            Some(TbAccount {
                identities: prefs.list(&format!("mail.account.{key}.identities")),
                key,
                server_key,
                server_type,
                name,
                host,
            })
        })
        .collect()
}

fn issue(
    account: &TbAccount,
    code: PortabilityIssueCode,
    severity: IssueSeverity,
    detail: impl Into<String>,
) -> ItemIssue {
    ItemIssue::new(
        code,
        severity,
        ItemRef {
            label: Some(account.name.clone()),
            ..ItemRef::default()
        },
        detail,
    )
}

fn tls_of(
    socket_type: Option<i64>,
    issues: &mut Vec<ItemIssue>,
    account: &TbAccount,
    side: &str,
) -> TlsMode {
    match socket_type {
        Some(3) => TlsMode::Implicit,
        Some(2) => TlsMode::Starttls,
        Some(1) => {
            issues.push(issue(
                account,
                PortabilityIssueCode::AccountNotExpressible,
                IssueSeverity::Warning,
                format!("{side}: \"STARTTLS if available\" becomes STARTTLS (Nail never falls back to plain text)"),
            ));
            TlsMode::Starttls
        }
        _ => {
            issues.push(issue(
                account,
                PortabilityIssueCode::AccountNotExpressible,
                IssueSeverity::Warning,
                format!("{side}: an unencrypted connection becomes STARTTLS"),
            ));
            TlsMode::Starttls
        }
    }
}

/// The OAuth2 provider Nail knows for a host.
fn oauth_provider(host: &str) -> Option<&'static str> {
    if nail_api::hosts::is_google_mail_host(host) {
        Some("google")
    } else if nail_api::hosts::is_microsoft_mail_host(host) {
        Some("microsoft")
    } else {
        None
    }
}

fn auth_of(
    method: Option<i64>,
    username: String,
    host: &str,
    issues: &mut Vec<ItemIssue>,
    account: &TbAccount,
    side: &str,
) -> AuthConfig {
    let password = |username: String| AuthConfig::Password {
        username,
        secret: None,
        mechanism: None,
    };
    match method {
        Some(1) => AuthConfig::None,
        Some(7) => AuthConfig::ClientCertificate {
            username: (!username.is_empty()).then_some(username),
            secret: None,
        },
        Some(10) => {
            if let Some(provider) = oauth_provider(host) {
                AuthConfig::OAuth2 {
                    username,
                    provider: provider.to_owned(),
                    secret: None,
                    mechanism: None,
                }
            } else {
                issues.push(issue(
                    account,
                    PortabilityIssueCode::AccountNotExpressible,
                    IssueSeverity::Warning,
                    format!("{side}: OAuth2 with an unknown provider becomes a password login"),
                ));
                password(username)
            }
        }
        Some(4..=6) => {
            issues.push(issue(
                account,
                PortabilityIssueCode::AccountNotExpressible,
                IssueSeverity::Warning,
                format!(
                    "{side}: authentication method {} becomes a password login",
                    method.unwrap_or(0)
                ),
            ));
            password(username)
        }
        _ => password(username),
    }
}

fn identity(prefs: &Prefs, key: &str) -> Option<Identity> {
    let p = |name: &str| format!("mail.identity.{key}.{name}");
    let email = prefs.text(&p("useremail"))?;
    let signature = prefs.text(&p("htmlSigText")).map(|text| {
        if prefs.bool(&p("htmlSigFormat")).unwrap_or(false) {
            Signature {
                text: None,
                html: Some(text),
                markdown: None,
            }
        } else {
            Signature {
                text: Some(text),
                html: None,
                markdown: None,
            }
        }
    });
    let bcc = if prefs.bool(&p("doBcc")).unwrap_or(false) {
        prefs.list(&p("doBccList"))
    } else {
        Vec::new()
    };
    Some(Identity {
        name: prefs.text(&p("fullName")),
        reply_to: prefs.text(&p("reply_to")),
        bcc,
        signature,
        ..Identity::new(AccountId::new(""), email)
    })
}

fn outgoing(
    prefs: &Prefs,
    smtp_key: &str,
    issues: &mut Vec<ItemIssue>,
    account: &TbAccount,
) -> Option<OutgoingServer> {
    let p = |name: &str| format!("mail.smtpserver.{smtp_key}.{name}");
    let host = prefs.text(&p("hostname"))?;
    let tls = tls_of(prefs.int(&p("try_ssl")), issues, account, "SMTP");
    let port = prefs
        .int(&p("port"))
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .unwrap_or_else(|| default_port(MailProtocol::Smtp, tls));
    let username = prefs.text(&p("username")).unwrap_or_default();
    let auth = auth_of(
        prefs.int(&p("authMethod")),
        username,
        &host,
        issues,
        account,
        "SMTP",
    );
    Some(OutgoingServer::Smtp {
        server: ServerEndpoint {
            host,
            port,
            tls,
            auth,
        },
        options: SmtpOptions::default(),
    })
}

/// An account proposal for an IMAP / POP3 account; `None` with an issue
/// for accounts Nail cannot take (Local Folders give `None` without one).
pub fn proposal(
    prefs: &Prefs,
    account: &TbAccount,
) -> Result<Option<AccountProposal>, Box<ItemIssue>> {
    let protocol = match account.server_type {
        ServerType::Imap => MailProtocol::Imap,
        ServerType::Pop3 => MailProtocol::Pop3,
        ServerType::None => return Ok(None),
        other => {
            return Err(Box::new(issue(
                account,
                PortabilityIssueCode::AccountNotExpressible,
                IssueSeverity::Skipped,
                format!("{other:?} accounts are not supported"),
            )));
        }
    };
    let s = |name: &str| format!("mail.server.{}.{name}", account.server_key);
    let mut issues = Vec::new();
    let identities: Vec<Identity> = account
        .identities
        .iter()
        .filter_map(|k| identity(prefs, k))
        .collect();
    let Some(email) = identities.first().map(|i| i.email.clone()) else {
        return Err(Box::new(issue(
            account,
            PortabilityIssueCode::AccountNotExpressible,
            IssueSeverity::Skipped,
            "the account has no identity with an address",
        )));
    };
    let Some(host) = account.host.clone() else {
        return Err(Box::new(issue(
            account,
            PortabilityIssueCode::AccountNotExpressible,
            IssueSeverity::Skipped,
            "the account has no server",
        )));
    };
    let tls = tls_of(
        prefs.int(&s("socketType")),
        &mut issues,
        account,
        "incoming",
    );
    let port = prefs
        .int(&s("port"))
        .and_then(|p| u16::try_from(p).ok())
        .filter(|p| *p != 0)
        .unwrap_or_else(|| default_port(protocol, tls));
    let username = prefs.text(&s("userName")).unwrap_or_else(|| email.clone());
    let auth = auth_of(
        prefs.int(&s("authMethod")),
        username,
        &host,
        &mut issues,
        account,
        "incoming",
    );
    let check_minutes = prefs
        .bool(&s("check_new_mail"))
        .unwrap_or(true)
        .then(|| prefs.int(&s("check_time")))
        .flatten()
        .and_then(|m| u32::try_from(m).ok())
        .filter(|m| *m > 0);
    let server = ServerEndpoint {
        host,
        port,
        tls,
        auth,
    };
    let incoming = match protocol {
        MailProtocol::Pop3 => {
            let leave = prefs.bool(&s("leave_on_server"));
            let delete_after_days = if prefs.bool(&s("delete_by_age_from_server")).unwrap_or(false)
            {
                prefs
                    .int(&s("num_days_to_leave_on_server"))
                    .and_then(|d| u32::try_from(d).ok())
            } else {
                None
            };
            IncomingServer::Pop3 {
                server,
                options: Pop3Options {
                    poll_interval_secs: check_minutes.map(|m| m * 60),
                    leave_on_server: leave,
                    delete_after_days,
                    delete_when_deleted_locally: prefs.bool(&s("deleteMailLeftOnServer")),
                },
            }
        }
        _ => IncomingServer::Imap {
            server,
            options: ImapOptions {
                poll_interval_secs: check_minutes.map(|m| m * 60),
                compress: None,
                max_connections: prefs
                    .int(&s("max_cached_connections"))
                    .and_then(|c| u32::try_from(c).ok())
                    .filter(|c| *c > 0),
            },
        },
    };
    let smtp_key = account
        .identities
        .first()
        .and_then(|id| prefs.text(&format!("mail.identity.{id}.smtpServer")))
        .or_else(|| prefs.text("mail.smtp.defaultserver"));
    let outgoing = smtp_key.and_then(|k| outgoing(prefs, &k, &mut issues, account));
    if account.identities.iter().any(|id| {
        prefs
            .text(&format!("mail.identity.{id}.sig_file"))
            .is_some()
    }) && identities.iter().all(|i| i.signature.is_none())
    {
        issues.push(issue(
            account,
            PortabilityIssueCode::AccountNotExpressible,
            IssueSeverity::Warning,
            "signature files are not imported",
        ));
    }
    issues.push(issue(
        account,
        PortabilityIssueCode::SecretNotImported,
        IssueSeverity::Warning,
        "passwords are never imported",
    ));
    Ok(Some(AccountProposal {
        key: account.key.clone(),
        config: AccountConfig {
            id: AccountId::new(""),
            name: account.name.clone(),
            email,
            enabled: true,
            incoming,
            outgoing,
            identities,
            sync: SyncPolicy::default(),
            save_sent_copy: None,
            authserv_ids: Vec::new(),
            sieve: None,
        },
        duplicate_of: None,
        issues,
    }))
}

/// Marks proposals whose login (incoming host + user name) matches an
/// existing account.
pub fn mark_duplicates(proposals: &mut [AccountProposal], existing: &[AccountConfig]) {
    fn login(config: &AccountConfig) -> (String, String) {
        let server = config.incoming.server();
        let user = match &server.auth {
            AuthConfig::Password { username, .. } | AuthConfig::OAuth2 { username, .. } => {
                username.clone()
            }
            AuthConfig::ClientCertificate { username, .. }
            | AuthConfig::Windows { username, .. } => username.clone().unwrap_or_default(),
            AuthConfig::None => config.email.clone(),
        };
        (server.host.to_ascii_lowercase(), user.to_ascii_lowercase())
    }
    for proposal in proposals {
        let wanted = login(&proposal.config);
        proposal.duplicate_of = existing
            .iter()
            .find(|a| login(a) == wanted || a.email.eq_ignore_ascii_case(&proposal.config.email))
            .map(|a| a.id.clone());
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn oauth_providers_need_a_domain_boundary() {
        assert_eq!(oauth_provider("imap.gmail.com"), Some("google"));
        assert_eq!(oauth_provider("outlook.office365.com"), Some("microsoft"));
        assert_eq!(oauth_provider("smtp-mail.outlook.com"), Some("microsoft"));
        // Copies of the rule once matched any suffix.
        assert_eq!(oauth_provider("notgmail.com"), None);
        assert_eq!(oauth_provider("mail.myoutlook.com"), None);
    }

    const PREFS: &str = r#"
user_pref("mail.accountmanager.accounts", "account1,account2,account3,account4");
user_pref("mail.account.account1.server", "server1");
user_pref("mail.account.account1.identities", "id1,id2");
user_pref("mail.server.server1.type", "imap");
user_pref("mail.server.server1.hostname", "imap.example.org");
user_pref("mail.server.server1.port", 993);
user_pref("mail.server.server1.socketType", 3);
user_pref("mail.server.server1.userName", "ada");
user_pref("mail.server.server1.name", "Work");
user_pref("mail.server.server1.authMethod", 3);
user_pref("mail.server.server1.check_time", 5);
user_pref("mail.identity.id1.useremail", "ada@example.org");
user_pref("mail.identity.id1.fullName", "Ada Lovelace");
user_pref("mail.identity.id1.smtpServer", "smtp1");
user_pref("mail.identity.id1.htmlSigText", "<b>Ada</b>");
user_pref("mail.identity.id1.htmlSigFormat", true);
user_pref("mail.identity.id2.useremail", "countess@example.org");
user_pref("mail.smtpserver.smtp1.hostname", "smtp.example.org");
user_pref("mail.smtpserver.smtp1.try_ssl", 2);
user_pref("mail.smtpserver.smtp1.username", "ada");
user_pref("mail.account.account2.server", "server2");
user_pref("mail.account.account2.identities", "id3");
user_pref("mail.server.server2.type", "pop3");
user_pref("mail.server.server2.hostname", "pop.gmail.com");
user_pref("mail.server.server2.socketType", 1);
user_pref("mail.server.server2.authMethod", 10);
user_pref("mail.server.server2.leave_on_server", true);
user_pref("mail.server.server2.delete_by_age_from_server", true);
user_pref("mail.server.server2.num_days_to_leave_on_server", 14);
user_pref("mail.identity.id3.useremail", "ada@gmail.com");
user_pref("mail.account.account3.server", "server3");
user_pref("mail.server.server3.type", "none");
user_pref("mail.server.server3.name", "Local Folders");
user_pref("mail.account.account4.server", "server4");
user_pref("mail.server.server4.type", "rss");
user_pref("mail.server.server4.name", "Blogs");
"#;

    #[test]
    fn imap_accounts_with_identities_and_smtp() {
        let prefs = Prefs::parse(PREFS);
        let all = accounts(&prefs);
        assert_eq!(all.len(), 4);
        assert_eq!(all[2].server_type, ServerType::None);
        let work = proposal(&prefs, &all[0]).unwrap().unwrap();
        assert_eq!(work.key, "account1");
        assert_eq!(work.config.name, "Work");
        assert_eq!(work.config.email, "ada@example.org");
        assert_eq!(work.config.identities.len(), 2);
        assert_eq!(
            work.config.identities[0].name.as_deref(),
            Some("Ada Lovelace")
        );
        assert_eq!(
            work.config.identities[0]
                .signature
                .as_ref()
                .unwrap()
                .html
                .as_deref(),
            Some("<b>Ada</b>")
        );
        let IncomingServer::Imap { server, options } = &work.config.incoming else {
            panic!("not imap");
        };
        assert_eq!(server.host, "imap.example.org");
        assert_eq!(server.port, 993);
        assert_eq!(server.tls, TlsMode::Implicit);
        assert!(
            matches!(&server.auth, AuthConfig::Password { username, secret: None, .. } if username == "ada")
        );
        assert_eq!(options.poll_interval_secs, Some(300));
        let Some(OutgoingServer::Smtp { server, .. }) = &work.config.outgoing else {
            panic!("no smtp");
        };
        assert_eq!(server.port, 587);
        assert_eq!(server.tls, TlsMode::Starttls);
        // Only the password note.
        assert_eq!(work.issues.len(), 1);
        assert_eq!(work.issues[0].code, PortabilityIssueCode::SecretNotImported);
        // Never a secret in the proposal.
        let json = serde_json::to_string(&work).unwrap();
        assert!(!json.contains("password\":\""));
    }

    #[test]
    fn pop_accounts_and_oauth() {
        let prefs = Prefs::parse(PREFS);
        let all = accounts(&prefs);
        let gmail = proposal(&prefs, &all[1]).unwrap().unwrap();
        let IncomingServer::Pop3 { server, options } = &gmail.config.incoming else {
            panic!("not pop");
        };
        assert!(
            matches!(&server.auth, AuthConfig::OAuth2 { provider, .. } if provider == "google")
        );
        assert_eq!(server.port, 110);
        assert_eq!(options.leave_on_server, Some(true));
        assert_eq!(options.delete_after_days, Some(14));
        assert!(gmail.config.outgoing.is_none());
        assert!(
            gmail
                .issues
                .iter()
                .any(|i| i.code == PortabilityIssueCode::AccountNotExpressible)
        );
    }

    #[test]
    fn local_folders_and_feeds() {
        let prefs = Prefs::parse(PREFS);
        let all = accounts(&prefs);
        assert_eq!(proposal(&prefs, &all[2]), Ok(None));
        let feeds = proposal(&prefs, &all[3]).unwrap_err();
        assert_eq!(feeds.severity, IssueSeverity::Skipped);
        assert_eq!(feeds.item.label.as_deref(), Some("Blogs"));
    }

    #[test]
    fn duplicates_by_login() {
        let prefs = Prefs::parse(PREFS);
        let all = accounts(&prefs);
        let mut proposals = vec![proposal(&prefs, &all[0]).unwrap().unwrap()];
        let mut existing = proposals[0].config.clone();
        existing.id = AccountId::new("work");
        existing.email = "other@example.org".into();
        mark_duplicates(&mut proposals, &[existing.clone()]);
        assert_eq!(proposals[0].duplicate_of, Some(AccountId::new("work")));
        existing.incoming = IncomingServer::Imap {
            server: ServerEndpoint {
                host: "imap.other.org".into(),
                port: 993,
                tls: TlsMode::Implicit,
                auth: AuthConfig::None,
            },
            options: ImapOptions::default(),
        };
        mark_duplicates(&mut proposals, &[existing]);
        assert_eq!(proposals[0].duplicate_of, None);
    }
}
