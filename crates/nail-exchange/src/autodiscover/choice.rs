//! Engine choice: Microsoft Graph for Exchange Online, EWS (with the found
//! URL) for Exchange Server on-premises, IMAP + SMTP as a fallback where
//! the server lists them. Builds the account proposals the setup dialog
//! shows (`DiscoveredAccount`, source `autodiscover`).

use nail_api::accounts::{
    AuthConfig, DiscoveredAccount, DiscoverySource, DiscoveryWarning, IncomingServer,
    ServerEndpoint, TlsMode,
};
use nail_api::exchange::{
    EWS_PATH, ExchangeEngine, ExchangeMailboxKind, ExchangeMailboxSettings, ExchangeOptions,
    GRAPH_HOST, is_exchange_online_host, is_loopback_host,
};
use serde::{Deserialize, Serialize};
use url::Url;

use super::pox::AlternativeMailbox;

/// The OAuth provider id of Microsoft accounts (`accounts.kdl`).
pub const MICROSOFT_PROVIDER: &str = "microsoft";

/// Which engine an EWS URL calls for.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineChoice {
    /// Graph for Exchange Online, EWS otherwise.
    pub engine: ExchangeEngine,
    /// The server is Exchange Online.
    pub online: bool,
    /// The EWS URL (EWS only).
    pub ews_url: Option<String>,
}

/// Chooses the engine for a mailbox whose EWS endpoint is `ews_url`:
/// Exchange Online (`outlook.office365.com`, …) → Graph, because Microsoft
/// is retiring EWS there (ADR 0075); anything else → EWS with that URL.
pub fn choose_engine(ews_url: &str) -> EngineChoice {
    let host = Url::parse(ews_url)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default();
    if is_exchange_online_host(&host) {
        EngineChoice {
            engine: ExchangeEngine::Graph,
            online: true,
            ews_url: None,
        }
    } else {
        EngineChoice {
            engine: ExchangeEngine::Ews,
            online: false,
            ews_url: Some(ews_url.to_owned()),
        }
    }
}

/// How the EWS proposal signs in.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EwsSignIn {
    /// The signed-in Windows user (Negotiate; nothing stored).
    WindowsUser,
    /// Windows sign-in with another user name (password from the
    /// keychain).
    WindowsAs(String),
    /// Basic over TLS with this user name.
    Basic(String),
}

/// The Graph proposal for `address` (OAuth with the Microsoft identity
/// platform, the Graph host).
pub fn graph_candidate(address: &str, provider_name: Option<String>) -> DiscoveredAccount {
    DiscoveredAccount {
        source: DiscoverySource::Autodiscover,
        incoming: IncomingServer::Exchange {
            server: ServerEndpoint {
                host: GRAPH_HOST.to_owned(),
                port: 443,
                tls: TlsMode::Implicit,
                auth: AuthConfig::OAuth2 {
                    username: address.to_owned(),
                    provider: MICROSOFT_PROVIDER.to_owned(),
                    secret: None,
                    mechanism: None,
                },
            },
            options: ExchangeOptions::for_engine(ExchangeEngine::Graph),
        },
        outgoing: None,
        provider_name,
        warnings: Vec::new(),
    }
}

/// The EWS proposal for `ews_url`; `None` for URLs that are neither
/// `https` nor loopback `http`.
pub fn ews_candidate(
    ews_url: &str,
    sign_in: &EwsSignIn,
    provider_name: Option<String>,
) -> Option<DiscoveredAccount> {
    let url = Url::parse(ews_url).ok()?;
    let host = url.host_str()?.to_ascii_lowercase();
    let tls = match url.scheme() {
        "https" => TlsMode::Implicit,
        "http" if is_loopback_host(&host) => TlsMode::Plaintext,
        _ => return None,
    };
    let port = url.port_or_known_default()?;
    let auth = match sign_in {
        EwsSignIn::WindowsUser => AuthConfig::Windows {
            username: None,
            secret: None,
            package: None,
        },
        EwsSignIn::WindowsAs(user) => AuthConfig::Windows {
            username: Some(user.clone()),
            secret: None,
            package: None,
        },
        EwsSignIn::Basic(user) => AuthConfig::Password {
            username: user.clone(),
            secret: None,
            mechanism: None,
        },
    };
    let default_url = format!("https://{host}{EWS_PATH}");
    let options = ExchangeOptions {
        url: (!ews_url.eq_ignore_ascii_case(&default_url)).then(|| ews_url.to_owned()),
        ..ExchangeOptions::for_engine(ExchangeEngine::Ews)
    };
    Some(DiscoveredAccount {
        source: DiscoverySource::Autodiscover,
        incoming: IncomingServer::Exchange {
            server: ServerEndpoint {
                host,
                port,
                tls,
                auth,
            },
            options,
        },
        outgoing: None,
        provider_name,
        warnings: Vec::new(),
    })
}

/// Whether `host` is the address's domain or below it.
pub fn under_domain(host: &str, domain: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    host == domain || host.ends_with(&format!(".{domain}"))
}

/// Marks proposals: `insecure-source` when a hop was plain HTTP,
/// `foreign-hosts` when an on-premises server is outside the address's
/// domain (Exchange Online and Graph are Microsoft's own hosts).
pub fn flag(candidates: &mut [DiscoveredAccount], domain: &str, insecure: bool) {
    for candidate in candidates {
        if insecure
            && !candidate
                .warnings
                .contains(&DiscoveryWarning::InsecureSource)
        {
            candidate.warnings.push(DiscoveryWarning::InsecureSource);
        }
        let mut hosts = vec![candidate.incoming.server().host.clone()];
        hosts.extend(candidate.outgoing.as_ref().map(|o| o.server().host.clone()));
        let foreign = hosts.iter().any(|h| {
            !under_domain(h, domain)
                && !is_exchange_online_host(h)
                && !h.eq_ignore_ascii_case(GRAPH_HOST)
                && !is_loopback_host(h)
        });
        if foreign && !candidate.warnings.contains(&DiscoveryWarning::ForeignHosts) {
            candidate.warnings.push(DiscoveryWarning::ForeignHosts);
        }
    }
}

/// The automapped mailboxes of a POX answer as mailbox settings (archives
/// and the user's own address left out). Autodiscover does not say whether
/// a mailbox is shared or another user's delegated one; both are shown as
/// shared mailboxes (the user can change the kind).
pub fn automapped(mailboxes: &[AlternativeMailbox], own: &str) -> Vec<ExchangeMailboxSettings> {
    let mut out: Vec<ExchangeMailboxSettings> = Vec::new();
    for mailbox in mailboxes {
        if mailbox.kind.eq_ignore_ascii_case("Archive")
            || mailbox.address.eq_ignore_ascii_case(own)
            || out.iter().any(|m| m.address == mailbox.address)
        {
            continue;
        }
        let mut settings = ExchangeMailboxSettings::shared(mailbox.address.clone());
        settings.name = mailbox.display_name.clone();
        settings.kind = ExchangeMailboxKind::Shared;
        if settings.issues(own).is_empty() {
            out.push(settings);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exchange_online_gets_graph() {
        let choice = choose_engine("https://outlook.office365.com/EWS/Exchange.asmx");
        assert_eq!(choice.engine, ExchangeEngine::Graph);
        assert!(choice.online && choice.ews_url.is_none());
        let choice = choose_engine("https://mail.corp.example/EWS/Exchange.asmx");
        assert_eq!(choice.engine, ExchangeEngine::Ews);
        assert_eq!(
            choice.ews_url.as_deref(),
            Some("https://mail.corp.example/EWS/Exchange.asmx")
        );
        assert!(!choice.online);
    }

    #[test]
    fn graph_candidate_uses_oauth() {
        let candidate = graph_candidate("ada@contoso.example", None);
        let IncomingServer::Exchange { server, options } = &candidate.incoming else {
            panic!()
        };
        assert_eq!(server.host, GRAPH_HOST);
        assert_eq!(options.engine, ExchangeEngine::Graph);
        assert!(
            options.issues(server).is_empty(),
            "{:?}",
            options.issues(server)
        );
        assert!(
            matches!(&server.auth, AuthConfig::OAuth2 { provider, username, .. } if provider == "microsoft" && username == "ada@contoso.example")
        );
    }

    #[test]
    fn ews_candidates() {
        let candidate = ews_candidate(
            "https://mail.corp.example/EWS/Exchange.asmx",
            &EwsSignIn::WindowsUser,
            Some("Ada".into()),
        )
        .unwrap();
        let IncomingServer::Exchange { server, options } = &candidate.incoming else {
            panic!()
        };
        assert_eq!(
            (server.host.as_str(), server.port),
            ("mail.corp.example", 443)
        );
        assert_eq!(options.url, None, "the default URL is not written");
        assert!(
            options.issues(server).is_empty(),
            "{:?}",
            options.issues(server)
        );
        assert!(matches!(
            server.auth,
            AuthConfig::Windows { username: None, .. }
        ));

        let custom = ews_candidate(
            "https://Mail.Corp.Example:8443/ews/exchange.asmx",
            &EwsSignIn::Basic("CORP\\ada".into()),
            None,
        )
        .unwrap();
        let IncomingServer::Exchange { server, options } = &custom.incoming else {
            panic!()
        };
        assert_eq!(
            (server.host.as_str(), server.port),
            ("mail.corp.example", 8443)
        );
        assert_eq!(
            options.url.as_deref(),
            Some("https://Mail.Corp.Example:8443/ews/exchange.asmx")
        );
        assert!(
            matches!(&server.auth, AuthConfig::Password { username, .. } if username == "CORP\\ada")
        );
        assert!(
            options.issues(server).is_empty(),
            "{:?}",
            options.issues(server)
        );

        let local = ews_candidate(
            "http://127.0.0.1:9000/EWS/Exchange.asmx",
            &EwsSignIn::WindowsAs("CORP\\ada".into()),
            None,
        )
        .unwrap();
        assert_eq!(local.incoming.server().tls, TlsMode::Plaintext);
        assert!(
            ews_candidate(
                "http://mail.corp.example/EWS/Exchange.asmx",
                &EwsSignIn::WindowsUser,
                None
            )
            .is_none()
        );
        assert!(ews_candidate("not a url", &EwsSignIn::WindowsUser, None).is_none());
    }

    #[test]
    fn flags_foreign_and_insecure() {
        let mut candidates = vec![
            ews_candidate(
                "https://mail.hoster.example/EWS/Exchange.asmx",
                &EwsSignIn::WindowsUser,
                None,
            )
            .unwrap(),
            ews_candidate(
                "https://mail.corp.example/EWS/Exchange.asmx",
                &EwsSignIn::WindowsUser,
                None,
            )
            .unwrap(),
            graph_candidate("ada@corp.example", None),
        ];
        flag(&mut candidates, "corp.example", false);
        assert_eq!(candidates[0].warnings, [DiscoveryWarning::ForeignHosts]);
        assert!(candidates[1].warnings.is_empty());
        assert!(candidates[2].warnings.is_empty());
        flag(&mut candidates, "corp.example", true);
        assert_eq!(candidates[1].warnings, [DiscoveryWarning::InsecureSource]);
        assert!(under_domain("Autodiscover.Corp.Example.", "corp.example"));
        assert!(!under_domain("evilcorp.example", "corp.example"));
    }

    #[test]
    fn automapped_mailboxes() {
        let found = automapped(
            &[
                AlternativeMailbox {
                    kind: "Delegate".into(),
                    display_name: Some("Support".into()),
                    address: "support@corp.example".into(),
                },
                AlternativeMailbox {
                    kind: "Archive".into(),
                    display_name: None,
                    address: "archive@corp.example".into(),
                },
                AlternativeMailbox {
                    kind: "TeamMailbox".into(),
                    display_name: None,
                    address: "ada@corp.example".into(),
                },
                AlternativeMailbox {
                    kind: "Delegate".into(),
                    display_name: None,
                    address: "support@corp.example".into(),
                },
            ],
            "ada@corp.example",
        );
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].address, "support@corp.example");
        assert_eq!(found[0].name.as_deref(), Some("Support"));
        assert_eq!(found[0].kind, ExchangeMailboxKind::Shared);
    }
}
