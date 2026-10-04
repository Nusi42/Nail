//! Engines for Exchange accounts, `TestAccount` and Autodiscover for
//! `DiscoverAccount` (ADR 0077).
//!
//! * **Engines**: Graph accounts get `GraphEngine::for_account` (OAuth2
//!   through the core's token manager); EWS accounts
//!   `EwsEngine::for_account` with Basic from the keychain, OAuth2, or
//!   Windows sign-in: `nail_exchange::sspi::SspiNegotiate` for the
//!   signed-in user or a user name with its keychain password (tests and
//!   demos inject a scripted `NegotiateAuth` through
//!   [`super::ExchangeHooks::negotiate`]).
//! * **Autodiscover**: `nail_exchange::autodiscover` runs next to
//!   nail-discovery's lookups; its Exchange proposals come first. It never
//!   sends a password: only the signed-in Windows user, and only when the
//!   request asks (`windows_sign_in`) and to hosts of the address's domain
//!   or ones the user trusts. A host outside the domain stops the run with
//!   a question ([`ExchangeDiscoveryPrompt::ConfirmRedirect`]). Microsoft's
//!   global Autodiscover endpoint (which learns the address) is asked only
//!   for domains whose MX is Microsoft's.

use std::sync::Arc;

use nail_api::accounts::{AuthConfig, DiscoveredAccount, IncomingServer, ServerEndpoint};
use nail_api::engine::{EngineError, SecretStore};
use nail_api::exchange::engine::{ExchangeEngine, NegotiateAuth};
use nail_api::exchange::{
    ExchangeDiscoveryInfo, ExchangeDiscoveryPrompt, ExchangeEngine as EngineKind,
    ExchangeRedirectReason,
};
use nail_api::{AccountConfig, ApiError, ErrorCode};
use nail_exchange::autodiscover::{
    AutodiscoverOutcome, AutodiscoverRequest, Autodiscoverer, RedirectReason,
};
use nail_exchange::negotiate::{HttpLogin, Password};
use nail_exchange::sspi::SspiNegotiate;
use nail_proto_ews::{EwsAuth, EwsClient, EwsEngine};
use nail_proto_graph::{GraphAuth, GraphClient, GraphEngine};

use super::{GraphEndpoint, exchange_parts};
use crate::creds::SecretSource;
use crate::error::{CoreError, Result};
use crate::service::Service;

/// Longest an Exchange Autodiscover run of `DiscoverAccount` takes.
const AUTODISCOVER_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(20);

fn engine_error(err: impl Into<EngineError>) -> CoreError {
    CoreError::Engine(err.into())
}

/// The Windows sign-in of an `auth "windows"` node: the injected one
/// (tests, demos), else SSPI with the signed-in user or the user name and
/// its password from `password`.
fn windows_login(
    service: &Service,
    auth: &AuthConfig,
    password: Option<String>,
) -> Result<Arc<dyn NegotiateAuth>> {
    if let Some(negotiate) = &service.inner.exchange.hooks.negotiate {
        return Ok(Arc::clone(negotiate));
    }
    let sspi = SspiNegotiate::from_auth(auth, password.map(Password::new))?;
    Ok(sspi.login().negotiate)
}

/// The keychain password of a Windows sign-in with a user name.
fn stored_password(
    secrets: &Arc<dyn SecretStore>,
    account: &AccountConfig,
    auth: &AuthConfig,
) -> Result<Option<String>> {
    let AuthConfig::Windows {
        username: Some(user),
        ..
    } = auth
    else {
        return Ok(None);
    };
    if user.trim().is_empty() {
        return Ok(None);
    }
    let Some(secret) = auth.secret_ref(&account.id) else {
        return Ok(None);
    };
    Ok(secrets.get(&secret)?)
}

/// The engine of a configured account (stored secrets).
pub(crate) fn engine_for(
    service: &Service,
    account: &AccountConfig,
) -> Result<Arc<dyn ExchangeEngine>> {
    let Some((server, options)) = exchange_parts(account) else {
        return Err(ApiError::invalid_request(format!(
            "account {} is not an Exchange account",
            account.id
        ))
        .into());
    };
    let tokens = Arc::clone(&service.inner.tokens);
    match options.engine {
        EngineKind::Graph => Ok(Arc::new(
            GraphEngine::for_account(account, tokens).map_err(engine_error)?,
        )),
        EngineKind::Ews => {
            let negotiate = match &server.auth {
                AuthConfig::Windows { .. } => {
                    let password = stored_password(&service.inner.secrets, account, &server.auth)?;
                    Some(windows_login(service, &server.auth, password)?)
                }
                _ => None,
            };
            Ok(Arc::new(
                EwsEngine::for_account(
                    account,
                    Arc::clone(&service.inner.secrets),
                    tokens,
                    negotiate,
                )
                .map_err(engine_error)?,
            ))
        }
    }
}

/// `TestAccount` for an Exchange account: sign in with the request's
/// secret (or the stored one) and ask the server who the mailbox is.
/// Answers capabilities like `exchange:graph`, `version:15.2…`.
pub(crate) async fn test_exchange(
    service: &Service,
    config: &AccountConfig,
    source: SecretSource,
) -> std::result::Result<Vec<String>, EngineError> {
    let Some((server, options)) = exchange_parts(config) else {
        return Err(EngineError::permanent(
            ErrorCode::InvalidRequest,
            "not an Exchange account",
        ));
    };
    let engine: Arc<dyn ExchangeEngine> = match source {
        SecretSource::Stored { .. } => engine_for(service, config).map_err(EngineError::from)?,
        SecretSource::AccessToken(token) => match options.engine {
            EngineKind::Graph => Arc::new(GraphEngine::new(
                GraphClient::new(&options.graph_url(server), GraphAuth::Bearer(token))
                    .map_err(EngineError::from)?,
            )),
            EngineKind::Ews => Arc::new(EwsEngine::new(
                EwsClient::new(&options.ews_url(server), EwsAuth::Bearer(token))
                    .map_err(EngineError::from)?
                    .with_own_address(&config.email),
                config.id.clone(),
            )),
        },
        SecretSource::Password(password) => {
            let auth = match &server.auth {
                AuthConfig::Password { username, .. } => EwsAuth::Basic {
                    username: username.clone(),
                    password: nail_proto_ews::auth::Password::Fixed(password),
                },
                AuthConfig::Windows { .. } => EwsAuth::Negotiate(
                    windows_login(service, &server.auth, Some(password))
                        .map_err(EngineError::from)?,
                ),
                _ => {
                    return Err(EngineError::permanent(
                        ErrorCode::InvalidRequest,
                        "a password does not sign in to this Exchange account",
                    ));
                }
            };
            if options.engine != EngineKind::Ews {
                return Err(EngineError::permanent(
                    ErrorCode::InvalidRequest,
                    "Graph accounts sign in with OAuth2",
                ));
            }
            Arc::new(EwsEngine::new(
                EwsClient::new(&options.ews_url(server), auth)
                    .map_err(EngineError::from)?
                    .with_own_address(&config.email),
                config.id.clone(),
            ))
        }
    };
    let info = engine.server_info().await?;
    let mut caps = vec![format!("exchange:{}", options.engine.as_str())];
    if let Some(version) = info.server_version {
        caps.push(format!("version:{version}"));
    }
    if info.focused_inbox {
        caps.push("focused-inbox".to_owned());
    }
    Ok(caps)
}

/// Points a Graph proposal at the configured endpoint (demos).
fn rewrite_graph(candidate: &mut DiscoveredAccount, endpoint: &GraphEndpoint) {
    if let IncomingServer::Exchange { server, options } = &mut candidate.incoming
        && options.engine == EngineKind::Graph
    {
        let username = match &server.auth {
            AuthConfig::OAuth2 { username, .. } => username.clone(),
            _ => String::new(),
        };
        *server = ServerEndpoint {
            host: endpoint.host.clone(),
            port: endpoint.port,
            tls: endpoint.tls,
            auth: AuthConfig::OAuth2 {
                username,
                provider: endpoint.provider.clone(),
                secret: None,
                mechanism: None,
            },
        };
    }
}

fn reason(reason: RedirectReason) -> ExchangeRedirectReason {
    match reason {
        RedirectReason::HttpRedirect => ExchangeRedirectReason::HttpRedirect,
        RedirectReason::RedirectUrl => ExchangeRedirectReason::RedirectUrl,
        RedirectReason::RedirectAddr => ExchangeRedirectReason::RedirectAddr,
        RedirectReason::HttpMethod => ExchangeRedirectReason::HttpMethod,
        RedirectReason::Srv => ExchangeRedirectReason::Srv,
    }
}

/// Exchange Autodiscover for `DiscoverAccount`: the Exchange proposals
/// (best first) and what to show besides them (`None` when Autodiscover
/// found nothing and asked nothing).
pub(crate) async fn autodiscover(
    service: &Service,
    email: &str,
    trusted_hosts: &[String],
    windows_sign_in: bool,
) -> (Vec<DiscoveredAccount>, Option<ExchangeDiscoveryInfo>) {
    let hooks = &service.inner.exchange.hooks;
    let http: Arc<dyn nail_exchange::http::ExchangeHttp> = match &hooks.autodiscover_http {
        Some(http) => Arc::clone(http),
        None => match nail_exchange::http::ReqwestHttp::system() {
            Ok(http) => Arc::new(http),
            Err(err) => {
                tracing::info!(%err, "no HTTP client for Autodiscover");
                return (Vec::new(), None);
            }
        },
    };
    // Shorter than a whole run of its own: it runs next to the other lookups.
    let mut discoverer = Autodiscoverer::new(http).with_timeout(AUTODISCOVER_TIMEOUT);
    let dns: Option<Arc<dyn nail_discovery::DnsLookup>> = match &hooks.autodiscover_dns {
        Some(dns) => Some(Arc::clone(dns)),
        None => nail_discovery::SystemDns::new()
            .ok()
            .map(|dns| -> Arc<dyn nail_discovery::DnsLookup> { Arc::new(dns) }),
    };
    // Microsoft's global endpoint learns the address: only for domains
    // whose mail Microsoft hosts already (their MX), never otherwise.
    let microsoft_hosted = match (&dns, email.rsplit_once('@')) {
        (Some(dns), Some((_, domain))) => dns
            .mx(domain.trim())
            .await
            .is_ok_and(|records| records.iter().any(|r| is_microsoft_mx(&r.exchange))),
        _ => false,
    };
    if hooks.no_microsoft_lookup || !microsoft_hosted {
        discoverer = discoverer.with_online_host(None);
    }
    if let Some(dns) = dns {
        discoverer = discoverer.with_dns(dns);
    }
    let login = if windows_sign_in {
        let auth = AuthConfig::Windows {
            username: None,
            secret: None,
            package: None,
        };
        match &hooks.negotiate {
            Some(negotiate) => HttpLogin::Windows(nail_exchange::negotiate::WindowsLogin {
                negotiate: Arc::clone(negotiate),
                ntlm: None,
            }),
            None => match SspiNegotiate::from_auth(&auth, None) {
                Ok(sspi) => HttpLogin::Windows(sspi.login()),
                Err(err) => {
                    tracing::info!(%err, "no Windows sign-in for Autodiscover");
                    HttpLogin::None
                }
            },
        }
    } else {
        HttpLogin::None
    };
    let request = AutodiscoverRequest {
        address: email.trim().to_owned(),
        login,
        windows_username: None,
        trusted_hosts: trusted_hosts.to_vec(),
        // A Windows sign-in goes only to hosts whose prompt the user
        // answered (ADR 0078).
        login_hosts: windows_sign_in.then(|| service.inner.exchange.login_hosts(email)),
    };
    let outcome = match discoverer.discover(&request).await {
        Ok(outcome) => outcome,
        Err(err) => {
            tracing::info!(%err, "Autodiscover failed");
            return (Vec::new(), None);
        }
    };
    match outcome {
        AutodiscoverOutcome::Found(found) => {
            let mut candidates = found.candidates.clone();
            if let Some(endpoint) = &hooks.graph_endpoint {
                for c in &mut candidates {
                    rewrite_graph(c, endpoint);
                }
            }
            service
                .inner
                .exchange
                .remember_discovery(email, found.mailboxes.clone());
            (
                candidates,
                Some(ExchangeDiscoveryInfo {
                    prompt: None,
                    display_name: found.display_name.clone(),
                    mailboxes: found.mailboxes.clone(),
                }),
            )
        }
        AutodiscoverOutcome::ConfirmRedirect(confirm) => (
            Vec::new(),
            Some(ExchangeDiscoveryInfo {
                prompt: Some(ExchangeDiscoveryPrompt::ConfirmRedirect {
                    localized: confirm.localized(),
                    url: confirm.url,
                    host: confirm.host,
                    domain: confirm.domain,
                    reason: reason(confirm.reason),
                }),
                ..ExchangeDiscoveryInfo::default()
            }),
        ),
        AutodiscoverOutcome::NeedsLogin(needed) => {
            service
                .inner
                .exchange
                .remember_login_prompt(email, &needed.host);
            (
                Vec::new(),
                Some(ExchangeDiscoveryInfo {
                    prompt: Some(ExchangeDiscoveryPrompt::NeedsLogin {
                        localized: needed.localized(),
                        host: needed.host,
                    }),
                    ..ExchangeDiscoveryInfo::default()
                }),
            )
        }
        AutodiscoverOutcome::NotFound { .. } => (Vec::new(), None),
    }
}

/// Whether a mail exchanger is Microsoft's (Exchange Online Protection).
fn is_microsoft_mx(host: &str) -> bool {
    let host = host.trim_end_matches('.').to_ascii_lowercase();
    ["mail.protection.outlook.com", "outlook.com", "hotmail.com"]
        .iter()
        .any(|d| host == *d || host.ends_with(&format!(".{d}")))
}

/// Stores the automapped mailboxes Autodiscover found for a new Exchange
/// account (`AddAccount`).
pub(crate) async fn store_discovered_mailboxes(service: &Service, account: &AccountConfig) {
    if exchange_parts(account).is_none() {
        return;
    }
    let found = service.inner.exchange.take_discovery(&account.email);
    if found.is_empty() {
        return;
    }
    let id = account.id.clone();
    let now = crate::engine::unix_now();
    if let Err(err) = service
        .store()
        .call(move |s| s.merge_automapped_mailboxes(&id, &found, now))
        .await
    {
        tracing::warn!(account = %account.id, %err, "cannot store the automapped mailboxes");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nail_api::accounts::TlsMode;

    #[test]
    fn graph_proposals_follow_the_demo_endpoint() {
        let mut candidate =
            nail_exchange::autodiscover::choice::graph_candidate("ada@contoso.example", None);
        let endpoint = GraphEndpoint {
            host: "127.0.0.1".into(),
            port: 4711,
            tls: TlsMode::Plaintext,
            provider: "demo-microsoft".into(),
        };
        rewrite_graph(&mut candidate, &endpoint);
        let IncomingServer::Exchange { server, .. } = &candidate.incoming else {
            panic!("an Exchange proposal");
        };
        assert_eq!((server.host.as_str(), server.port), ("127.0.0.1", 4711));
        let AuthConfig::OAuth2 {
            username, provider, ..
        } = &server.auth
        else {
            panic!("OAuth2");
        };
        assert_eq!(username, "ada@contoso.example");
        assert_eq!(provider, "demo-microsoft");
    }

    #[test]
    fn microsoft_mail_exchangers_are_recognized() {
        assert!(is_microsoft_mx("contoso-com.mail.protection.outlook.com."));
        assert!(is_microsoft_mx("eur.olc.protection.outlook.com"));
        assert!(!is_microsoft_mx("mx.example.org"));
        assert!(!is_microsoft_mx("notoutlook.com"));
    }

    #[test]
    fn redirect_reasons_map_one_to_one() {
        for (from, to) in [
            (
                RedirectReason::HttpRedirect,
                ExchangeRedirectReason::HttpRedirect,
            ),
            (
                RedirectReason::RedirectUrl,
                ExchangeRedirectReason::RedirectUrl,
            ),
            (
                RedirectReason::RedirectAddr,
                ExchangeRedirectReason::RedirectAddr,
            ),
            (
                RedirectReason::HttpMethod,
                ExchangeRedirectReason::HttpMethod,
            ),
            (RedirectReason::Srv, ExchangeRedirectReason::Srv),
        ] {
            assert_eq!(reason(from), to);
        }
    }
}
