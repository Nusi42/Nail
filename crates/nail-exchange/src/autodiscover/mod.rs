//! Account setup through Autodiscover with engine choice (plan M3c WP93).
//!
//! [`Autodiscoverer::discover`] takes an address and answers one of:
//!
//! * [`AutodiscoverOutcome::Found`]: ranked proposals — Microsoft Graph for
//!   Exchange Online, EWS with the found URL for Exchange Server
//!   on-premises, IMAP + SMTP where the server lists them — plus the
//!   automapped shared mailboxes and the user's display name;
//! * [`AutodiscoverOutcome::ConfirmRedirect`]: the next step would send the
//!   login to a server outside the address's domain. Nothing was sent; the
//!   UI asks the user and runs again with that host in
//!   [`AutodiscoverRequest::trusted_hosts`];
//! * [`AutodiscoverOutcome::NeedsLogin`]: a server of the domain asks to
//!   sign in and no login was given;
//! * [`AutodiscoverOutcome::NotFound`].
//!
//! Order (SCP lookup in Active Directory is out of scope):
//!
//! 1. **Autodiscover v2** (JSON, no login) at `autodiscover.<domain>`,
//!    `<domain>` and Microsoft's global endpoint
//!    ([`v2::ONLINE_V2_HOST`], can be switched off): an EWS URL on
//!    Exchange Online means Graph; any other means EWS, and the POX answer
//!    of that server (with the login, only when the host is trusted) adds
//!    the display name and automapped mailboxes.
//! 2. **POX** (with the login) at `https://<domain>/autodiscover/
//!    autodiscover.xml` and `https://autodiscover.<domain>/…`, following
//!    `redirectAddr` (a v2 lookup first for the new address, which needs
//!    no login), `redirectUrl` and HTTP redirects (`https` only).
//! 3. The **HTTP redirect method** (`GET http://autodiscover.<domain>/…`
//!    answering a redirect to an `https` URL) and **SRV**
//!    `_autodiscover._tcp.<domain>` (through the OS resolver): both are
//!    unauthenticated pointers, so their targets always need the user's
//!    confirmation unless already trusted.
//!
//! **Credentials never go to a host outside the address's domain**
//! (`host == domain` or below it) unless the user confirmed that host;
//! every hop is counted ([`MAX_HOPS`]) and only `https` targets are
//! followed. The same login also never travels over plain HTTP (Basic is
//! refused there; Windows sign-in goes only where the host is trusted).

pub mod choice;
pub mod pox;
pub mod v2;

use std::fmt;
use std::sync::Arc;
use std::time::Duration;

use nail_api::ErrorCode;
use nail_api::accounts::{DiscoveredAccount, DiscoverySource};
use nail_api::exchange::{ExchangeMailboxSettings, is_loopback_host};
use nail_api::localized::LocalizedMessage;
use nail_discovery::DnsLookup;
use serde::Serialize;
use url::Url;

use self::choice::{
    EngineChoice, EwsSignIn, automapped, choose_engine, ews_candidate, flag, graph_candidate,
    under_domain,
};
use self::pox::{PoxAnswer, PoxSettings};
use crate::http::{ExchangeHttp, HttpRequest, HttpResponse};
use crate::negotiate::{
    AuthScheme, HttpLogin, SignInError, answer_challenge, challenges_of, offered_schemes,
};
use crate::texts;

/// Most redirects (HTTP, `redirectUrl`, `redirectAddr`) of one run.
pub const MAX_HOPS: usize = 10;

/// Default time for a whole run (the protocol clients' default).
pub use nail_net::DEFAULT_TIMEOUT;

/// What to discover.
#[derive(Debug, Clone, Default)]
pub struct AutodiscoverRequest {
    /// The email address.
    pub address: String,
    /// How to answer servers that ask to sign in (POX). `None` = never
    /// send credentials (only v2 and unauthenticated answers are used).
    pub login: HttpLogin,
    /// The user name of a Windows login for another user (`DOMAIN\user`);
    /// `None` with a Windows login = the signed-in Windows user. Only used
    /// to fill the EWS proposal.
    pub windows_username: Option<String>,
    /// Hosts outside the address's domain the user confirmed
    /// ([`AutodiscoverOutcome::ConfirmRedirect`]).
    pub trusted_hosts: Vec<String>,
    /// Hosts that may get the login: the ones the user answered a
    /// [`AutodiscoverOutcome::NeedsLogin`] prompt for. Another host asking
    /// to sign in is a new prompt (a Windows sign-in is not offered to
    /// every host of the domain, ADR 0078). `None` = every trusted host.
    pub login_hosts: Option<Vec<String>>,
}

/// Why a host needs confirmation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum RedirectReason {
    /// An HTTP redirect (`Location`) to another domain.
    HttpRedirect,
    /// A POX `redirectUrl` to another domain.
    RedirectUrl,
    /// A POX `redirectAddr` to an address of another domain.
    RedirectAddr,
    /// The unauthenticated HTTP redirect method
    /// (`http://autodiscover.<domain>`).
    HttpMethod,
    /// A DNS SRV record (`_autodiscover._tcp`).
    Srv,
}

/// The next step would send the login to `host`: the UI asks the user.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RedirectConfirmation {
    /// The URL that would get the login.
    pub url: String,
    /// Its host (what the user confirms; goes to
    /// [`AutodiscoverRequest::trusted_hosts`]).
    pub host: String,
    /// The address's domain.
    pub domain: String,
    /// How the host was found.
    pub reason: RedirectReason,
}

impl RedirectConfirmation {
    /// The question to show (`exchange-autodiscover-confirm-redirect`).
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(texts::AUTODISCOVER_CONFIRM_REDIRECT)
            .arg("host", self.host.clone())
            .arg("domain", self.domain.clone())
    }
}

/// A server of the domain asks to sign in and no login was given.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct LoginNeeded {
    /// The URL that asked.
    pub url: String,
    /// Its host.
    pub host: String,
    /// The offered sign-in methods.
    pub schemes: Vec<AuthScheme>,
}

impl LoginNeeded {
    /// The prompt (`exchange-autodiscover-needs-login`).
    pub fn localized(&self) -> LocalizedMessage {
        LocalizedMessage::new(texts::AUTODISCOVER_NEEDS_LOGIN).arg("host", self.host.clone())
    }
}

/// What Autodiscover found.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ExchangeDiscovery {
    /// The address the settings are for (another one after
    /// `redirectAddr`).
    pub address: String,
    /// The user's display name, when a POX answer gave it.
    pub display_name: Option<String>,
    /// The engine chosen.
    pub choice: EngineChoice,
    /// Proposals, best first: the Exchange engine, then IMAP + SMTP.
    pub candidates: Vec<DiscoveredAccount>,
    /// Automapped shared mailboxes (origin `automapped` when stored).
    pub mailboxes: Vec<ExchangeMailboxSettings>,
    /// The URLs asked, in order (diagnostics; never credentials).
    pub trace: Vec<String>,
}

impl ExchangeDiscovery {
    /// The summary to show (`exchange-autodiscover-online` /
    /// `exchange-autodiscover-on-premises`).
    pub fn localized(&self) -> LocalizedMessage {
        if self.choice.online {
            LocalizedMessage::new(texts::AUTODISCOVER_ONLINE)
        } else {
            let host = self
                .choice
                .ews_url
                .as_deref()
                .and_then(|u| Url::parse(u).ok())
                .and_then(|u| u.host_str().map(str::to_owned))
                .unwrap_or_default();
            LocalizedMessage::new(texts::AUTODISCOVER_ON_PREMISES).arg("host", host)
        }
    }
}

/// The answer of a run.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "type", rename_all = "kebab-case")]
pub enum AutodiscoverOutcome {
    /// Settings found.
    Found(Box<ExchangeDiscovery>),
    /// Ask the user before sending the login to another host.
    ConfirmRedirect(RedirectConfirmation),
    /// A server wants a login.
    NeedsLogin(LoginNeeded),
    /// Nothing answered.
    NotFound {
        /// The URLs asked.
        trace: Vec<String>,
    },
}

/// A run that cannot continue.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum AutodiscoverError {
    /// The address is not an email address.
    #[error("not an email address")]
    InvalidAddress,
    /// More than [`MAX_HOPS`] redirects.
    #[error("too many Autodiscover redirects")]
    TooManyRedirects,
    /// Signing in failed (wrong password, SSPI error).
    #[error("{0}")]
    SignIn(SignInError),
}

impl AutodiscoverError {
    /// The error class.
    pub fn code(&self) -> ErrorCode {
        match self {
            Self::InvalidAddress => ErrorCode::InvalidRequest,
            Self::TooManyRedirects => ErrorCode::Network,
            Self::SignIn(e) => e.code(),
        }
    }

    /// The user-facing text.
    pub fn localized(&self) -> LocalizedMessage {
        match self {
            Self::InvalidAddress => LocalizedMessage::new(texts::AUTODISCOVER_INVALID_ADDRESS),
            Self::TooManyRedirects => LocalizedMessage::new(texts::AUTODISCOVER_TOO_MANY_REDIRECTS),
            Self::SignIn(e) => e.localized(),
        }
    }
}

/// Splits an address into (address lowercased domain kept, domain).
fn parse_address(address: &str) -> Option<(String, String)> {
    let address = address.trim();
    let (local, domain) = address.rsplit_once('@')?;
    let domain = domain.trim_end_matches('.').to_ascii_lowercase();
    let valid_domain = !domain.is_empty()
        && domain.contains('.')
        && domain
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '.')
        && !domain.starts_with(['.', '-']);
    if local.is_empty() || local.contains(char::is_whitespace) || !valid_domain {
        return None;
    }
    Some((format!("{local}@{domain}"), domain))
}

/// Autodiscover for Exchange accounts (see the module docs).
#[derive(Clone)]
pub struct Autodiscoverer {
    http: Arc<dyn ExchangeHttp>,
    dns: Option<Arc<dyn DnsLookup>>,
    online_host: Option<String>,
    timeout: Duration,
}

impl fmt::Debug for Autodiscoverer {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Autodiscoverer")
            .field("online_host", &self.online_host)
            .field("srv", &self.dns.is_some())
            .field("timeout", &self.timeout)
            .finish_non_exhaustive()
    }
}

impl Autodiscoverer {
    /// Autodiscover over `http`, with Microsoft's global v2 endpoint and
    /// without SRV lookups.
    pub fn new(http: Arc<dyn ExchangeHttp>) -> Self {
        Self {
            http,
            dns: None,
            online_host: Some(v2::ONLINE_V2_HOST.to_owned()),
            timeout: DEFAULT_TIMEOUT,
        }
    }

    /// Also looks up `_autodiscover._tcp.<domain>` (use
    /// `nail_discovery::SystemDns`: the OS resolver, ADR 0057).
    #[must_use]
    pub fn with_dns(mut self, dns: Arc<dyn DnsLookup>) -> Self {
        self.dns = Some(dns);
        self
    }

    /// The global v2 host to ask last (`None` = do not ask Microsoft).
    #[must_use]
    pub fn with_online_host(mut self, host: Option<String>) -> Self {
        self.online_host = host;
        self
    }

    /// Time for a whole run.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Runs Autodiscover (see the module docs).
    pub async fn discover(
        &self,
        request: &AutodiscoverRequest,
    ) -> Result<AutodiscoverOutcome, AutodiscoverError> {
        let (address, domain) =
            parse_address(&request.address).ok_or(AutodiscoverError::InvalidAddress)?;
        let mut run = Run {
            d: self,
            request,
            domain,
            hops: 0,
            insecure: false,
            trace: Vec::new(),
        };
        if let Ok(result) = tokio::time::timeout(self.timeout, run.start(&address)).await {
            result
        } else {
            tracing::debug!("Autodiscover timed out");
            Ok(AutodiscoverOutcome::NotFound { trace: run.trace })
        }
    }
}

/// Where a POX URL came from (decides whether it may get the login).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// Built from the address's own domain.
    Domain,
    /// A redirect (HTTP, `redirectUrl`) or another address's domain.
    Redirect(RedirectReason),
    /// An unauthenticated pointer (HTTP method, SRV).
    Pointer(RedirectReason),
}

type Step = Result<Option<AutodiscoverOutcome>, AutodiscoverError>;

struct Run<'a> {
    d: &'a Autodiscoverer,
    request: &'a AutodiscoverRequest,
    domain: String,
    hops: usize,
    insecure: bool,
    trace: Vec<String>,
}

impl Run<'_> {
    fn hop(&mut self) -> Result<(), AutodiscoverError> {
        self.hops += 1;
        if self.hops > MAX_HOPS {
            Err(AutodiscoverError::TooManyRedirects)
        } else {
            Ok(())
        }
    }

    fn confirmed(&self, host: &str) -> bool {
        self.request.trusted_hosts.iter().any(|h| {
            h.trim_end_matches('.')
                .eq_ignore_ascii_case(host.trim_end_matches('.'))
        })
    }

    /// Whether `url` may get the login.
    fn trusted(&self, url: &Url, origin: Origin) -> bool {
        let host = url.host_str().unwrap_or_default();
        if self.confirmed(host) {
            return true;
        }
        match origin {
            Origin::Domain | Origin::Redirect(_) => under_domain(host, &self.domain),
            Origin::Pointer(_) => false,
        }
    }

    fn reason(origin: Origin) -> RedirectReason {
        match origin {
            Origin::Domain => RedirectReason::HttpRedirect,
            Origin::Redirect(r) | Origin::Pointer(r) => r,
        }
    }

    fn ews_sign_in(&self) -> EwsSignIn {
        match &self.request.login {
            HttpLogin::Basic { username, .. } => EwsSignIn::Basic(username.clone()),
            HttpLogin::Windows(_) | HttpLogin::None => match &self.request.windows_username {
                Some(user) => EwsSignIn::WindowsAs(user.clone()),
                None => EwsSignIn::WindowsUser,
            },
        }
    }

    async fn start(&mut self, address: &str) -> Result<AutodiscoverOutcome, AutodiscoverError> {
        if let Some(ews_url) = self.v2_lookup(address).await? {
            return self.settle_ews_url(address, &ews_url).await;
        }
        let domain = self.domain.clone();
        if let Some(outcome) = Box::pin(self.pox_flow(address, &domain, Origin::Domain)).await? {
            return Ok(outcome);
        }
        Ok(AutodiscoverOutcome::NotFound {
            trace: std::mem::take(&mut self.trace),
        })
    }

    /// `GET` without credentials, following `https` redirects.
    async fn get_following(
        &mut self,
        mut url: Url,
    ) -> Result<Option<HttpResponse>, AutodiscoverError> {
        loop {
            self.trace.push(url.to_string());
            let response = match self.d.http.send(&HttpRequest::get(url.clone())).await {
                Ok(response) => response,
                Err(error) => {
                    tracing::debug!(%url, %error, "Autodiscover v2 failed");
                    return Ok(None);
                }
            };
            if !response.is_redirect() {
                return Ok(Some(response));
            }
            self.hop()?;
            match response
                .header_value("Location")
                .and_then(|l| url.join(l).ok())
            {
                Some(next) if next.scheme() == "https" => url = next,
                _ => return Ok(None),
            }
        }
    }

    /// The EWS URL of Autodiscover v2, asking the domain's hosts, then
    /// Microsoft's.
    async fn v2_lookup(&mut self, address: &str) -> Result<Option<String>, AutodiscoverError> {
        let domain = address.rsplit_once('@').map(|(_, d)| d).unwrap_or_default();
        let mut hosts = vec![format!("autodiscover.{domain}"), domain.to_owned()];
        hosts.extend(self.d.online_host.clone());
        for host in hosts {
            let Some(url) = v2::url(&host, address, "EWS") else {
                continue;
            };
            let Some(response) = self.get_following(url).await? else {
                continue;
            };
            if response.status != 200 {
                continue;
            }
            match v2::parse_answer(&response.body, "EWS") {
                Ok(ews) => return Ok(Some(ews)),
                Err(error) => tracing::debug!(%host, %error, "no Autodiscover v2 answer"),
            }
        }
        Ok(None)
    }

    /// A result from a v2 EWS URL (on-premises: details from the server's
    /// POX answer when the host is trusted and a login is given).
    async fn settle_ews_url(
        &mut self,
        address: &str,
        ews_url: &str,
    ) -> Result<AutodiscoverOutcome, AutodiscoverError> {
        let choice = choose_engine(ews_url);
        let mut details = PoxSettings::default();
        if !choice.online && self.request.login.is_some() {
            let pox_url = Url::parse(ews_url)
                .ok()
                .and_then(|u| u.join("/autodiscover/autodiscover.xml").ok());
            if let Some(pox_url) = pox_url
                && self.trusted(&pox_url, Origin::Redirect(RedirectReason::HttpRedirect))
            {
                match self.pox_request(&pox_url, address).await {
                    Ok(Some(PoxAnswer::Settings(settings))) => details = *settings,
                    Ok(_) => {}
                    Err(AutodiscoverError::SignIn(error))
                        if error.code() == ErrorCode::AuthFailed =>
                    {
                        return Err(AutodiscoverError::SignIn(error));
                    }
                    Err(error) => tracing::debug!(%error, "no POX details"),
                }
            }
        }
        details.internal_ews_url = None;
        details.external_ews_url = Some(ews_url.to_owned());
        Ok(self
            .found(address, &details)
            .unwrap_or(AutodiscoverOutcome::NotFound {
                trace: std::mem::take(&mut self.trace),
            }))
    }

    /// One authenticated POX request (no redirects followed; `None` for
    /// answers that are not POX).
    async fn pox_request(
        &mut self,
        url: &Url,
        address: &str,
    ) -> Result<Option<PoxAnswer>, AutodiscoverError> {
        self.trace.push(url.to_string());
        let request = HttpRequest::post(
            url.clone(),
            "text/xml; charset=utf-8",
            pox::request_body(address).into_bytes(),
        );
        let Ok(first) = self.d.http.send(&request).await else {
            return Ok(None);
        };
        let response = answer_challenge(self.d.http.as_ref(), request, first, &self.request.login)
            .await
            .map_err(AutodiscoverError::SignIn)?;
        if response.status != 200 {
            return Ok(None);
        }
        Ok(pox::parse_response(&response.body, address).ok())
    }

    /// POX at the domain's two URLs, then the HTTP redirect method and SRV.
    async fn pox_flow(&mut self, address: &str, domain: &str, origin: Origin) -> Step {
        for url in [
            format!("https://{domain}/autodiscover/autodiscover.xml"),
            format!("https://autodiscover.{domain}/autodiscover/autodiscover.xml"),
        ] {
            let Ok(url) = Url::parse(&url) else { continue };
            if let Some(outcome) = Box::pin(self.pox_at(url, address, origin)).await? {
                return Ok(Some(outcome));
            }
        }
        if let Some(target) = self.http_redirect_method(domain).await? {
            self.insecure = true;
            let pointer = Origin::Pointer(RedirectReason::HttpMethod);
            if let Some(outcome) = Box::pin(self.pox_at(target, address, pointer)).await? {
                return Ok(Some(outcome));
            }
        }
        for url in self.srv_targets(domain).await {
            let pointer = Origin::Pointer(RedirectReason::Srv);
            if let Some(outcome) = Box::pin(self.pox_at(url, address, pointer)).await? {
                return Ok(Some(outcome));
            }
        }
        Ok(None)
    }

    /// `GET http://autodiscover.<domain>/autodiscover/autodiscover.xml`
    /// answering a redirect to an `https` URL.
    async fn http_redirect_method(
        &mut self,
        domain: &str,
    ) -> Result<Option<Url>, AutodiscoverError> {
        let Ok(url) = Url::parse(&format!(
            "http://autodiscover.{domain}/autodiscover/autodiscover.xml"
        )) else {
            return Ok(None);
        };
        self.trace.push(url.to_string());
        let Ok(response) = self.d.http.send(&HttpRequest::get(url.clone())).await else {
            return Ok(None);
        };
        if !response.is_redirect() {
            return Ok(None);
        }
        self.hop()?;
        Ok(response
            .header_value("Location")
            .and_then(|l| url.join(l).ok())
            .filter(|u| u.scheme() == "https"))
    }

    /// POX URLs from `_autodiscover._tcp.<domain>`, best first.
    async fn srv_targets(&self, domain: &str) -> Vec<Url> {
        let Some(dns) = &self.d.dns else {
            return Vec::new();
        };
        let name = format!("_autodiscover._tcp.{domain}");
        let mut records = match dns.srv(&name).await {
            Ok(records) => records,
            Err(error) => {
                tracing::debug!(%name, %error, "no Autodiscover SRV records");
                return Vec::new();
            }
        };
        records.retain(|r| !r.target.is_empty() && r.target != ".");
        records.sort_by(|a, b| a.priority.cmp(&b.priority).then(b.weight.cmp(&a.weight)));
        records
            .into_iter()
            .filter_map(|r| {
                let target = r.target.trim_end_matches('.');
                let url = if r.port == nail_net::ports::HTTPS {
                    format!("https://{target}/autodiscover/autodiscover.xml")
                } else {
                    format!("https://{target}:{}/autodiscover/autodiscover.xml", r.port)
                };
                Url::parse(&url).ok()
            })
            .collect()
    }

    /// POX at one URL: unauthenticated first; a `401` gets the login only
    /// when the host is trusted.
    async fn pox_at(&mut self, url: Url, address: &str, origin: Origin) -> Step {
        let host = url.host_str().unwrap_or_default().to_ascii_lowercase();
        if url.scheme() != "https" && !is_loopback_host(&host) {
            return Ok(None);
        }
        self.trace.push(url.to_string());
        let request = HttpRequest::post(
            url.clone(),
            "text/xml; charset=utf-8",
            pox::request_body(address).into_bytes(),
        );
        let first = match self.d.http.send(&request).await {
            Ok(response) => response,
            Err(error) => {
                tracing::debug!(%url, %error, "Autodiscover POX failed");
                return Ok(None);
            }
        };
        let response = if first.status == 401 {
            if !self.trusted(&url, origin) {
                return Ok(Some(AutodiscoverOutcome::ConfirmRedirect(
                    RedirectConfirmation {
                        url: url.to_string(),
                        host,
                        domain: self.domain.clone(),
                        reason: Self::reason(origin),
                    },
                )));
            }
            let allowed = self
                .request
                .login_hosts
                .as_ref()
                .is_none_or(|hosts| hosts.iter().any(|h| h.eq_ignore_ascii_case(&host)));
            if !self.request.login.is_some() || !allowed {
                return Ok(Some(AutodiscoverOutcome::NeedsLogin(LoginNeeded {
                    url: url.to_string(),
                    host,
                    schemes: offered_schemes(&challenges_of(&first)),
                })));
            }
            answer_challenge(self.d.http.as_ref(), request, first, &self.request.login)
                .await
                .map_err(AutodiscoverError::SignIn)?
        } else {
            first
        };
        if response.is_redirect() {
            self.hop()?;
            let next = response
                .header_value("Location")
                .and_then(|l| url.join(l).ok())
                .filter(|u| u.scheme() == "https");
            let Some(next) = next else { return Ok(None) };
            let origin = match origin {
                Origin::Pointer(r) => Origin::Pointer(r),
                _ => Origin::Redirect(RedirectReason::HttpRedirect),
            };
            return Box::pin(self.pox_at(next, address, origin)).await;
        }
        if response.status != 200 {
            return Ok(None);
        }
        match pox::parse_response(&response.body, address) {
            Ok(PoxAnswer::Settings(settings)) => Ok(self.found(address, &settings)),
            Ok(PoxAnswer::RedirectUrl(target)) => {
                self.hop()?;
                match Url::parse(&target) {
                    Ok(next) if next.scheme() == "https" => {
                        let origin = match origin {
                            Origin::Pointer(r) => Origin::Pointer(r),
                            _ => Origin::Redirect(RedirectReason::RedirectUrl),
                        };
                        Box::pin(self.pox_at(next, address, origin)).await
                    }
                    _ => Ok(None),
                }
            }
            Ok(PoxAnswer::RedirectAddr(next)) => {
                self.hop()?;
                let Some((next, next_domain)) = parse_address(&next) else {
                    return Ok(None);
                };
                if next.eq_ignore_ascii_case(address) {
                    return Ok(None);
                }
                // The new address may be in Exchange Online (hybrid): v2
                // needs no login.
                if let Some(ews_url) = self.v2_lookup(&next).await? {
                    return self.settle_ews_url(&next, &ews_url).await.map(Some);
                }
                let origin = if under_domain(&next_domain, &self.domain) {
                    Origin::Domain
                } else {
                    Origin::Redirect(RedirectReason::RedirectAddr)
                };
                Box::pin(self.pox_flow(&next, &next_domain, origin)).await
            }
            Err(error) => {
                tracing::debug!(%url, %error, "unusable Autodiscover answer");
                Ok(None)
            }
        }
    }

    /// The outcome for POX settings (`None` when nothing is usable).
    fn found(&mut self, address: &str, settings: &PoxSettings) -> Option<AutodiscoverOutcome> {
        let mut candidates: Vec<DiscoveredAccount> = Vec::new();
        let choice = settings.ews_url().map(choose_engine);
        if let Some(choice) = &choice {
            if choice.online {
                candidates.push(graph_candidate(address, settings.display_name.clone()));
            } else if let Some(candidate) = choice.ews_url.as_deref().and_then(|url| {
                ews_candidate(url, &self.ews_sign_in(), settings.display_name.clone())
            }) {
                candidates.push(candidate);
            }
        }
        for incoming in &settings.incoming {
            candidates.push(DiscoveredAccount {
                source: DiscoverySource::Autodiscover,
                incoming: incoming.clone(),
                outgoing: settings.outgoing.clone(),
                provider_name: settings.display_name.clone(),
                warnings: Vec::new(),
            });
        }
        if candidates.is_empty() {
            return None;
        }
        flag(&mut candidates, &self.domain, self.insecure);
        let choice = choice.unwrap_or(EngineChoice {
            engine: nail_api::exchange::ExchangeEngine::Ews,
            online: false,
            ews_url: None,
        });
        Some(AutodiscoverOutcome::Found(Box::new(ExchangeDiscovery {
            address: address.to_owned(),
            display_name: settings.display_name.clone(),
            choice,
            candidates,
            mailboxes: automapped(&settings.mailboxes, address),
            trace: std::mem::take(&mut self.trace),
        })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn addresses() {
        assert_eq!(
            parse_address(" Ada@Corp.Example. "),
            Some(("Ada@corp.example".into(), "corp.example".into()))
        );
        assert_eq!(parse_address("ada"), None);
        assert_eq!(parse_address("@corp.example"), None);
        assert_eq!(parse_address("ada@localhost"), None);
        assert_eq!(parse_address("ada@corp example"), None);
        assert_eq!(parse_address("a b@corp.example"), None);
    }

    #[test]
    fn errors_and_texts() {
        assert_eq!(
            AutodiscoverError::InvalidAddress.code(),
            ErrorCode::InvalidRequest
        );
        assert_eq!(
            AutodiscoverError::TooManyRedirects.localized().id,
            texts::AUTODISCOVER_TOO_MANY_REDIRECTS
        );
        assert_eq!(
            AutodiscoverError::SignIn(SignInError::Rejected).code(),
            ErrorCode::AuthFailed
        );
        let confirm = RedirectConfirmation {
            url: "https://mail.hoster.example/autodiscover/autodiscover.xml".into(),
            host: "mail.hoster.example".into(),
            domain: "corp.example".into(),
            reason: RedirectReason::Srv,
        };
        let text = confirm.localized();
        assert_eq!(text.id, texts::AUTODISCOVER_CONFIRM_REDIRECT);
        assert_eq!(text.args.len(), 2);
        let json = serde_json::to_value(AutodiscoverOutcome::ConfirmRedirect(confirm)).unwrap();
        assert_eq!(json["type"], "confirm-redirect");
        assert_eq!(json["reason"], "srv");
    }
}
