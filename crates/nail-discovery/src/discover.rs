//! [`Discoverer`]: runs every source for an address and ranks the results.

use std::sync::Arc;
use std::time::Duration;

use nail_api::accounts::{
    AuthConfig, DiscoveredAccount, DiscoverySource, ImapOptions, IncomingServer, MailProtocol,
    OutgoingServer, Pop3Options, ServerEndpoint, SmtpOptions, TlsMode,
};
use url::Url;

use crate::autoconfig::parse_client_config;
use crate::autodiscover::{Answer, parse_response, request_body};
use crate::email::{Email, base_domain};
use crate::net::{
    DnsLookup, HickoryDns, HostProbe, HttpFetch, NetError, ReqwestFetch, SrvRecord, TcpProbe,
};

/// Mozilla's ISP database.
pub const ISPDB_BASE: &str = "https://autoconfig.thunderbird.net/v1.1/";

/// Why discovery could not run.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[non_exhaustive]
pub enum DiscoveryError {
    /// The text is not a plausible email address.
    #[error("not an email address: {0:?}")]
    InvalidEmail(String),
    /// Setting up the network clients failed.
    #[error("discovery setup: {0}")]
    Setup(String),
}

impl DiscoveryError {
    /// The API error code.
    pub fn code(&self) -> nail_api::ErrorCode {
        match self {
            Self::InvalidEmail(_) => nail_api::ErrorCode::InvalidRequest,
            Self::Setup(_) => nail_api::ErrorCode::Internal,
        }
    }
}

/// Everything discovery needs: the network seams and settings.
#[derive(Debug, Clone)]
pub struct Discoverer {
    http: Arc<dyn HttpFetch>,
    dns: Arc<dyn DnsLookup>,
    probe: Arc<dyn HostProbe>,
    /// Base URL of the ISPDB; the domain is appended.
    pub ispdb_base: Url,
    /// Time limit of each source.
    pub timeout: Duration,
    /// Probe common host names when no other source answered.
    pub guess: bool,
}

impl Discoverer {
    /// Discovery over the given seams, with Mozilla's ISPDB, 10 s per
    /// source and guessing enabled.
    pub fn new(
        http: Arc<dyn HttpFetch>,
        dns: Arc<dyn DnsLookup>,
        probe: Arc<dyn HostProbe>,
    ) -> Self {
        Self {
            http,
            dns,
            probe,
            ispdb_base: Url::parse(ISPDB_BASE).expect("constant URL"),
            timeout: Duration::from_secs(10),
            guess: true,
        }
    }

    /// Real network access: `http` (e.g. `nail_auth::http_client()`), the
    /// system's DNS resolver and TCP probes.
    pub fn system(http: reqwest::Client) -> Result<Self, DiscoveryError> {
        let dns = HickoryDns::system().map_err(|e| DiscoveryError::Setup(e.0))?;
        Ok(Self::new(
            Arc::new(ReqwestFetch::new(http)),
            Arc::new(dns),
            Arc::new(TcpProbe::new(Duration::from_secs(5))),
        ))
    }

    /// Sets the ISPDB base URL.
    #[must_use]
    pub fn with_ispdb_base(mut self, base: Url) -> Self {
        self.ispdb_base = base;
        self
    }

    /// Sets the per-source timeout.
    #[must_use]
    pub fn with_timeout(mut self, timeout: Duration) -> Self {
        self.timeout = timeout;
        self
    }

    /// Enables or disables host guessing.
    #[must_use]
    pub fn with_guessing(mut self, guess: bool) -> Self {
        self.guess = guess;
        self
    }

    /// Discovers settings for `email`; see [`discover`].
    pub async fn discover(&self, email: &str) -> Result<Vec<DiscoveredAccount>, DiscoveryError> {
        discover(email, self).await
    }

    async fn bounded(
        &self,
        source: &str,
        fut: impl Future<Output = Vec<DiscoveredAccount>>,
    ) -> Vec<DiscoveredAccount> {
        match tokio::time::timeout(self.timeout, fut).await {
            Ok(found) => found,
            Err(_) => {
                tracing::debug!(source, "discovery source timed out");
                Vec::new()
            }
        }
    }

    /// GET with up to three redirects (to `https` targets only, after the
    /// first hop), returning a 200 body.
    async fn fetch(&self, url: &Url) -> Result<Vec<u8>, NetError> {
        let mut url = url.clone();
        for _ in 0..4 {
            let reply = self.http.get(&url).await?;
            match reply.status {
                200 => return Ok(reply.body),
                301 | 302 | 303 | 307 | 308 => {
                    let location = reply
                        .location
                        .ok_or_else(|| NetError("redirect without Location".into()))?;
                    let next = url.join(&location).map_err(|e| NetError(e.to_string()))?;
                    if next.scheme() != "https" {
                        return Err(NetError("refusing a redirect to plain HTTP".into()));
                    }
                    url = next;
                }
                status => return Err(NetError(format!("HTTP {status}"))),
            }
        }
        Err(NetError("too many redirects".into()))
    }

    async fn client_config(
        &self,
        url: Url,
        email: &Email,
        source: DiscoverySource,
    ) -> Vec<DiscoveredAccount> {
        let found = match self.fetch(&url).await {
            Ok(body) => parse_client_config(&body, email, source).map_err(NetError),
            Err(e) => Err(e),
        };
        match found {
            Ok(found) => found,
            Err(err) => {
                tracing::debug!(%url, %err, "autoconfig lookup found nothing");
                Vec::new()
            }
        }
    }

    /// The first of `urls` that yields candidates.
    async fn first_client_config(
        &self,
        urls: Vec<Url>,
        email: &Email,
        source: DiscoverySource,
    ) -> Vec<DiscoveredAccount> {
        for url in urls {
            let found = self.client_config(url, email, source).await;
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    async fn autoconfig(&self, email: &Email) -> Vec<DiscoveredAccount> {
        let query = url::form_urlencoded::Serializer::new(String::new())
            .append_pair("emailaddress", &email.address)
            .finish();
        let urls = ["https", "http"]
            .iter()
            .filter_map(|scheme| {
                Url::parse(&format!(
                    "{scheme}://autoconfig.{}/mail/config-v1.1.xml?{query}",
                    email.domain
                ))
                .ok()
            })
            .collect();
        self.first_client_config(urls, email, DiscoverySource::Autoconfig)
            .await
    }

    async fn well_known(&self, email: &Email) -> Vec<DiscoveredAccount> {
        let urls = ["https", "http"]
            .iter()
            .filter_map(|scheme| {
                Url::parse(&format!(
                    "{scheme}://{}/.well-known/autoconfig/mail/config-v1.1.xml",
                    email.domain
                ))
                .ok()
            })
            .collect();
        self.first_client_config(urls, email, DiscoverySource::WellKnown)
            .await
    }

    fn ispdb_url(&self, domain: &str) -> Option<Url> {
        self.ispdb_base.join(domain).ok()
    }

    /// The ISPDB entry of the domain, else of the domain of its MX host
    /// (custom domains hosted by Google, Microsoft, …).
    async fn ispdb(&self, email: &Email) -> Vec<DiscoveredAccount> {
        if let Some(url) = self.ispdb_url(&email.domain) {
            let found = self.client_config(url, email, DiscoverySource::Ispdb).await;
            if !found.is_empty() {
                return found;
            }
        }
        let mut mx = match self.dns.mx(&email.domain).await {
            Ok(mx) => mx,
            Err(err) => {
                tracing::debug!(%err, "MX lookup failed");
                return Vec::new();
            }
        };
        mx.sort_by_key(|m| m.preference);
        let Some(best) = mx.into_iter().find(|m| !m.exchange.is_empty()) else {
            return Vec::new();
        };
        let base = base_domain(&best.exchange);
        if base == email.domain {
            return Vec::new();
        }
        match self.ispdb_url(&base) {
            Some(url) => self.client_config(url, email, DiscoverySource::Ispdb).await,
            None => Vec::new(),
        }
    }

    async fn best_srv(&self, service: &str, domain: &str) -> Option<SrvRecord> {
        let name = format!("{service}._tcp.{domain}");
        let mut records = match self.dns.srv(&name).await {
            Ok(records) => records,
            Err(err) => {
                tracing::debug!(%name, %err, "SRV lookup failed");
                return None;
            }
        };
        // RFC 2782: lowest priority, then highest weight (a deterministic
        // choice is fine for a setup proposal).
        records.sort_by(|a, b| a.priority.cmp(&b.priority).then(b.weight.cmp(&a.weight)));
        records
            .into_iter()
            .next()
            .filter(|r| !r.target.is_empty() && r.port != 0)
    }

    /// RFC 6186 (and RFC 8314 `_submissions`).
    async fn srv(&self, email: &Email) -> Vec<DiscoveredAccount> {
        let domain = &email.domain;
        let (imaps, imap, pop3s, pop3, submissions, submission) = tokio::join!(
            self.best_srv("_imaps", domain),
            self.best_srv("_imap", domain),
            self.best_srv("_pop3s", domain),
            self.best_srv("_pop3", domain),
            self.best_srv("_submissions", domain),
            self.best_srv("_submission", domain),
        );
        let endpoint = |r: &SrvRecord, tls| ServerEndpoint {
            host: r.target.clone(),
            port: r.port,
            tls,
            auth: password(email),
        };
        let outgoing = submissions
            .map(|r| endpoint(&r, TlsMode::Implicit))
            .or_else(|| submission.map(|r| endpoint(&r, TlsMode::Starttls)))
            .map(|server| OutgoingServer::Smtp {
                server,
                options: SmtpOptions::default(),
            });
        let incoming = [
            imaps.map(|r| imap_server(endpoint(&r, TlsMode::Implicit))),
            imap.map(|r| imap_server(endpoint(&r, TlsMode::Starttls))),
            pop3s.map(|r| pop3_server(endpoint(&r, TlsMode::Implicit))),
            pop3.map(|r| pop3_server(endpoint(&r, TlsMode::Starttls))),
        ];
        incoming
            .into_iter()
            .flatten()
            .map(|incoming| DiscoveredAccount {
                source: DiscoverySource::Srv,
                incoming,
                outgoing: outgoing.clone(),
                provider_name: None,
            })
            .collect()
    }

    async fn autodiscover_at(
        &self,
        url: &Url,
        email: &Email,
        depth: usize,
    ) -> Vec<DiscoveredAccount> {
        let body = request_body(&email.address).into_bytes();
        let reply = match self.http.post(url, "text/xml; charset=utf-8", body).await {
            Ok(reply) if reply.status == 200 => reply,
            Ok(reply) => {
                tracing::debug!(%url, status = reply.status, "Autodiscover found nothing");
                return Vec::new();
            }
            Err(err) => {
                tracing::debug!(%url, %err, "Autodiscover failed");
                return Vec::new();
            }
        };
        match parse_response(&reply.body, email) {
            Ok(Answer::Settings(found)) => found,
            Ok(Answer::RedirectAddr(address)) if depth < 2 => match Email::parse(&address) {
                Some(next) if next.address != email.address => {
                    Box::pin(self.autodiscover_email(&next, depth + 1))
                        .await
                        .into_iter()
                        .map(|mut a| {
                            // The settings are for the redirected address; the
                            // login stays as the server said.
                            a.source = DiscoverySource::Autodiscover;
                            a
                        })
                        .collect()
                }
                _ => Vec::new(),
            },
            Ok(Answer::RedirectUrl(target)) if depth < 2 => match Url::parse(&target) {
                Ok(next) if next.scheme() == "https" => {
                    Box::pin(self.autodiscover_at(&next, email, depth + 1)).await
                }
                _ => Vec::new(),
            },
            Ok(_) => Vec::new(),
            Err(err) => {
                tracing::debug!(%url, %err, "unusable Autodiscover response");
                Vec::new()
            }
        }
    }

    async fn autodiscover_email(&self, email: &Email, depth: usize) -> Vec<DiscoveredAccount> {
        let urls = [
            format!(
                "https://autodiscover.{}/autodiscover/autodiscover.xml",
                email.domain
            ),
            format!("https://{}/autodiscover/autodiscover.xml", email.domain),
        ];
        for url in urls.iter().filter_map(|u| Url::parse(u).ok()) {
            let found = self.autodiscover_at(&url, email, depth).await;
            if !found.is_empty() {
                return found;
            }
        }
        Vec::new()
    }

    /// Probes common host names; one candidate per incoming protocol found.
    async fn guess(&self, email: &Email) -> Vec<DiscoveredAccount> {
        let d = &email.domain;
        let imap = [
            (format!("imap.{d}"), 993, TlsMode::Implicit),
            (format!("mail.{d}"), 993, TlsMode::Implicit),
            (format!("imap.{d}"), 143, TlsMode::Starttls),
            (format!("mail.{d}"), 143, TlsMode::Starttls),
        ];
        let pop3 = [
            (format!("pop.{d}"), 995, TlsMode::Implicit),
            (format!("pop3.{d}"), 995, TlsMode::Implicit),
            (format!("mail.{d}"), 995, TlsMode::Implicit),
            (format!("pop.{d}"), 110, TlsMode::Starttls),
        ];
        let smtp = [
            (format!("smtp.{d}"), 465, TlsMode::Implicit),
            (format!("mail.{d}"), 465, TlsMode::Implicit),
            (format!("smtp.{d}"), 587, TlsMode::Starttls),
            (format!("mail.{d}"), 587, TlsMode::Starttls),
        ];
        let (imap, pop3, smtp) = tokio::join!(
            self.first_reachable(&imap),
            self.first_reachable(&pop3),
            self.first_reachable(&smtp)
        );
        let endpoint = |(host, port, tls): (String, u16, TlsMode)| ServerEndpoint {
            host,
            port,
            tls,
            auth: password(email),
        };
        let outgoing = smtp.map(|s| OutgoingServer::Smtp {
            server: endpoint(s),
            options: SmtpOptions::default(),
        });
        [
            imap.map(|s| imap_server(endpoint(s))),
            pop3.map(|s| pop3_server(endpoint(s))),
        ]
        .into_iter()
        .flatten()
        .map(|incoming| DiscoveredAccount {
            source: DiscoverySource::Guess,
            incoming,
            outgoing: outgoing.clone(),
            provider_name: None,
        })
        .collect()
    }

    /// The first candidate (in preference order) that answers; all are
    /// probed at once.
    async fn first_reachable(
        &self,
        candidates: &[(String, u16, TlsMode)],
    ) -> Option<(String, u16, TlsMode)> {
        let probes = candidates.iter().map(|(host, port, _)| {
            let probe = Arc::clone(&self.probe);
            let host = host.clone();
            let port = *port;
            tokio::spawn(async move { probe.reachable(&host, port).await })
        });
        let handles: Vec<_> = probes.collect();
        for (candidate, handle) in candidates.iter().zip(handles) {
            if handle.await.unwrap_or(false) {
                return Some(candidate.clone());
            }
        }
        None
    }
}

fn password(email: &Email) -> AuthConfig {
    // Big providers only accept OAuth2 (or app passwords); prefer it.
    AuthConfig::Password {
        username: email.address.clone(),
        secret: None,
        mechanism: None,
    }
}

fn with_known_oauth(mut endpoint: ServerEndpoint) -> ServerEndpoint {
    if let (AuthConfig::Password { username, .. }, Some(provider)) =
        (&endpoint.auth, nail_auth::provider_for_host(&endpoint.host))
    {
        endpoint.auth = AuthConfig::OAuth2 {
            username: username.clone(),
            provider: provider.to_owned(),
            secret: None,
            mechanism: None,
        };
    }
    endpoint
}

fn imap_server(server: ServerEndpoint) -> IncomingServer {
    IncomingServer::Imap {
        server: with_known_oauth(server),
        options: ImapOptions::default(),
    }
}

fn pop3_server(server: ServerEndpoint) -> IncomingServer {
    IncomingServer::Pop3 {
        server: with_known_oauth(server),
        options: Pop3Options::default(),
    }
}

fn source_rank(source: DiscoverySource) -> u8 {
    match source {
        DiscoverySource::Autoconfig => 0,
        DiscoverySource::WellKnown => 1,
        DiscoverySource::Ispdb => 2,
        DiscoverySource::Srv => 3,
        DiscoverySource::Autodiscover => 4,
        DiscoverySource::Guess => 5,
    }
}

/// Whether any part of a candidate is unencrypted.
fn plaintext(candidate: &DiscoveredAccount) -> bool {
    candidate.incoming.server().tls == TlsMode::Plaintext
        || candidate
            .outgoing
            .as_ref()
            .is_some_and(|o| o.server().tls == TlsMode::Plaintext)
}

/// Orders candidates: encrypted before unencrypted, then by source (most
/// trusted first), keeping each source's own order; drops duplicates
/// (same servers from several sources), keeping the best-ranked one.
pub fn rank(candidates: Vec<DiscoveredAccount>) -> Vec<DiscoveredAccount> {
    let mut indexed: Vec<(usize, DiscoveredAccount)> = candidates.into_iter().enumerate().collect();
    indexed.sort_by_key(|(i, c)| (plaintext(c), source_rank(c.source), *i));
    let mut out: Vec<DiscoveredAccount> = Vec::new();
    for (_, candidate) in indexed {
        let duplicate = out
            .iter()
            .any(|o| o.incoming == candidate.incoming && o.outgoing == candidate.outgoing);
        if !duplicate {
            out.push(candidate);
        }
    }
    out
}

/// Finds account settings for `email`.
///
/// Runs autoconfig (`autoconfig.<domain>`, `https` then `http`),
/// `/.well-known/autoconfig`, the ISPDB (by domain, else by MX host), RFC 6186
/// SRV records and Autodiscover at once, each within
/// [`Discoverer::timeout`]; if all of them come back empty and guessing is
/// on, probes common host names. Returns the [`rank`]ed candidates; an
/// empty list means "enter the settings manually". Every candidate's
/// authentication has the user name filled in and no secret; hosts of
/// Google and Microsoft get OAuth2.
pub async fn discover(
    email: &str,
    discoverer: &Discoverer,
) -> Result<Vec<DiscoveredAccount>, DiscoveryError> {
    let email =
        Email::parse(email).ok_or_else(|| DiscoveryError::InvalidEmail(email.to_owned()))?;
    let d = discoverer;
    let (autoconfig, well_known, ispdb, srv, autodiscover) = tokio::join!(
        d.bounded("autoconfig", d.autoconfig(&email)),
        d.bounded("well-known", d.well_known(&email)),
        d.bounded("ispdb", d.ispdb(&email)),
        d.bounded("srv", d.srv(&email)),
        d.bounded("autodiscover", d.autodiscover_email(&email, 0)),
    );
    let mut all: Vec<DiscoveredAccount> = [autoconfig, well_known, ispdb, srv, autodiscover]
        .into_iter()
        .flatten()
        .collect();
    if all.is_empty() && d.guess {
        all = d.bounded("guess", d.guess(&email)).await;
    }
    let ranked = rank(all);
    tracing::debug!(domain = %email.domain, candidates = ranked.len(), "discovery finished");
    Ok(ranked)
}

/// Which protocols a candidate uses, for display and tests.
pub fn protocols(candidate: &DiscoveredAccount) -> (MailProtocol, Option<MailProtocol>) {
    let incoming = match candidate.incoming {
        IncomingServer::Imap { .. } => MailProtocol::Imap,
        IncomingServer::Pop3 { .. } => MailProtocol::Pop3,
    };
    (
        incoming,
        candidate.outgoing.as_ref().map(|_| MailProtocol::Smtp),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn candidate(source: DiscoverySource, host: &str, tls: TlsMode) -> DiscoveredAccount {
        DiscoveredAccount {
            source,
            incoming: IncomingServer::Imap {
                server: ServerEndpoint {
                    host: host.into(),
                    port: 993,
                    tls,
                    auth: AuthConfig::None,
                },
                options: ImapOptions::default(),
            },
            outgoing: None,
            provider_name: None,
        }
    }

    #[test]
    fn ranking_prefers_encryption_then_source_and_dedupes() {
        let ranked = rank(vec![
            candidate(DiscoverySource::Guess, "a", TlsMode::Implicit),
            candidate(DiscoverySource::Autoconfig, "b", TlsMode::Plaintext),
            candidate(DiscoverySource::Srv, "c", TlsMode::Implicit),
            candidate(DiscoverySource::Ispdb, "c", TlsMode::Implicit),
            candidate(DiscoverySource::Autoconfig, "d", TlsMode::Starttls),
        ]);
        let order: Vec<(&str, DiscoverySource)> = ranked
            .iter()
            .map(|c| (c.incoming.server().host.as_str(), c.source))
            .collect();
        assert_eq!(
            order,
            [
                ("d", DiscoverySource::Autoconfig),
                ("c", DiscoverySource::Ispdb),
                ("a", DiscoverySource::Guess),
                ("b", DiscoverySource::Autoconfig),
            ]
        );
        assert_eq!(protocols(&ranked[0]), (MailProtocol::Imap, None));
    }

    #[test]
    fn known_hosts_get_oauth() {
        let email = Email::parse("ada@gmail.com").unwrap();
        let server = with_known_oauth(ServerEndpoint {
            host: "imap.gmail.com".into(),
            port: 993,
            tls: TlsMode::Implicit,
            auth: password(&email),
        });
        assert!(
            matches!(server.auth, AuthConfig::OAuth2 { ref provider, .. } if provider == "google")
        );
        assert_eq!(
            DiscoveryError::InvalidEmail("x".into()).code(),
            nail_api::ErrorCode::InvalidRequest
        );
    }
}
