//! The network seams of discovery: HTTP, DNS and TCP reachability, each a
//! trait with a real implementation, so tests (and end-to-end runs against
//! mock servers) never touch the network.

use std::collections::HashMap;
use std::fmt;
use std::net::{IpAddr, SocketAddr};
use std::time::Duration;

use hickory_resolver::config::{
    ConnectionConfig, NameServerConfig, ProtocolConfig, ResolverConfig,
};
use hickory_resolver::net::runtime::TokioRuntimeProvider;
use hickory_resolver::proto::rr::RData;
use hickory_resolver::{Resolver, TokioResolver};
use nail_api::engine::BoxFuture;
use url::Url;

use crate::xml::MAX_DOCUMENT;

/// A failed network operation (English diagnostic).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct NetError(pub String);

/// An HTTP response.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HttpReply {
    /// Status code.
    pub status: u16,
    /// `Location` header, for redirects.
    pub location: Option<String>,
    /// Body (at most 1 MiB).
    pub body: Vec<u8>,
}

/// HTTP access for discovery.
pub trait HttpFetch: Send + Sync + fmt::Debug {
    /// `GET url` without following redirects.
    fn get<'a>(&'a self, url: &'a Url) -> BoxFuture<'a, Result<HttpReply, NetError>>;
    /// `POST url` with a body, without following redirects.
    fn post<'a>(
        &'a self,
        url: &'a Url,
        content_type: &'a str,
        body: Vec<u8>,
    ) -> BoxFuture<'a, Result<HttpReply, NetError>>;
}

/// [`HttpFetch`] on a [`reqwest::Client`] (build it with
/// `nail_auth::http_client()`: rustls, platform verifier, no redirects).
#[derive(Debug, Clone)]
pub struct ReqwestFetch {
    client: reqwest::Client,
    route_all_to: Option<SocketAddr>,
}

impl ReqwestFetch {
    /// Fetches with `client`.
    pub fn new(client: reqwest::Client) -> Self {
        Self {
            client,
            route_all_to: None,
        }
    }

    /// Sends every request over plain HTTP to `addr` instead, keeping the
    /// original host in the `Host` header and the path and query as they
    /// are. For tests and end-to-end runs against a mock HTTP server that
    /// routes by host (`nail_testkit_net::MockHttpServer`).
    #[must_use]
    pub fn route_all_to(mut self, addr: SocketAddr) -> Self {
        self.route_all_to = Some(addr);
        self
    }

    fn target(&self, url: &Url) -> (Url, Option<String>) {
        nail_net::http::route(url, self.route_all_to, false)
    }

    async fn run(
        &self,
        request: reqwest::RequestBuilder,
        host: Option<String>,
    ) -> Result<HttpReply, NetError> {
        let request = match host {
            Some(host) => request.header(reqwest::header::HOST, host),
            None => request,
        };
        let mut response = request
            .send()
            .await
            .map_err(|e| NetError(e.without_url().to_string()))?;
        let status = response.status().as_u16();
        let location = response
            .headers()
            .get(reqwest::header::LOCATION)
            .and_then(|l| l.to_str().ok())
            .map(str::to_owned);
        let body = nail_net::http::read_limited(&mut response, MAX_DOCUMENT)
            .await
            .map_err(|e| match e {
                nail_net::http::BodyError::TooLarge => NetError("response too large".to_owned()),
                nail_net::http::BodyError::Transport(t) => NetError(t.to_string()),
            })?;
        Ok(HttpReply {
            status,
            location,
            body,
        })
    }
}

impl HttpFetch for ReqwestFetch {
    fn get<'a>(&'a self, url: &'a Url) -> BoxFuture<'a, Result<HttpReply, NetError>> {
        Box::pin(async move {
            let (target, host) = self.target(url);
            self.run(self.client.get(target), host).await
        })
    }

    fn post<'a>(
        &'a self,
        url: &'a Url,
        content_type: &'a str,
        body: Vec<u8>,
    ) -> BoxFuture<'a, Result<HttpReply, NetError>> {
        Box::pin(async move {
            let (target, host) = self.target(url);
            let request = self
                .client
                .post(target)
                .header(reqwest::header::CONTENT_TYPE, content_type)
                .body(body);
            self.run(request, host).await
        })
    }
}

/// An SRV record (RFC 2782).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SrvRecord {
    /// Lower is preferred.
    pub priority: u16,
    /// Relative weight among equal priorities.
    pub weight: u16,
    /// Port.
    pub port: u16,
    /// Target host without trailing dot; empty for "service not available".
    pub target: String,
}

/// An MX record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MxRecord {
    /// Lower is preferred.
    pub preference: u16,
    /// Mail exchanger host without trailing dot.
    pub exchange: String,
}

/// DNS lookups for discovery. A name without records is `Ok(vec![])`.
pub trait DnsLookup: Send + Sync + fmt::Debug {
    /// SRV records of `name` (e.g. `_imaps._tcp.example.org`).
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>>;
    /// MX records of `domain`.
    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>>;
}

/// [`DnsLookup`] with `hickory-resolver`.
#[derive(Clone)]
pub struct HickoryDns {
    resolver: TokioResolver,
}

impl fmt::Debug for HickoryDns {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("HickoryDns").finish_non_exhaustive()
    }
}

fn fqdn(name: &str) -> String {
    format!("{}.", name.trim_end_matches('.'))
}

fn host(name: &hickory_resolver::proto::rr::Name) -> String {
    name.to_string().trim_end_matches('.').to_ascii_lowercase()
}

impl HickoryDns {
    /// The system's resolver configuration. Not on Windows: hickory binds
    /// its UDP sockets to the unspecified address, which the firewall
    /// prompts for; Windows uses [`crate::SystemDns`] (ADR 0057).
    #[cfg(not(windows))]
    pub fn system() -> Result<Self, NetError> {
        let resolver = Resolver::builder_tokio()
            .map_err(|e| NetError(e.to_string()))?
            .build()
            .map_err(|e| NetError(e.to_string()))?;
        Ok(Self { resolver })
    }

    /// A resolver asking only the loopback name server at `addr` over UDP
    /// (tests, end-to-end runs with `nail_testkit_net::MockDnsServer`). Its
    /// sockets bind the same loopback address, never the unspecified one
    /// (ADR 0057); any other name server is refused.
    pub fn with_name_server(addr: SocketAddr, timeout: Duration) -> Result<Self, NetError> {
        let server = loopback_name_server(addr)?;
        let mut builder = Resolver::builder_with_config(
            ResolverConfig::from_name_servers(vec![server]),
            TokioRuntimeProvider::default(),
        );
        builder.options_mut().timeout = timeout;
        builder.options_mut().attempts = 1;
        let resolver = builder.build().map_err(|e| NetError(e.to_string()))?;
        Ok(Self { resolver })
    }

    async fn records(&self, name: &str, srv: bool) -> Result<Vec<RData>, NetError> {
        let lookup = if srv {
            self.resolver.srv_lookup(fqdn(name)).await
        } else {
            self.resolver.mx_lookup(fqdn(name)).await
        };
        match lookup {
            Ok(lookup) => Ok(lookup.answers().iter().map(|r| r.data.clone()).collect()),
            Err(e) if e.is_no_records_found() || e.is_nx_domain() => Ok(Vec::new()),
            Err(e) => Err(NetError(e.to_string())),
        }
    }
}

/// The configuration of a loopback name server: UDP from a random port of
/// the same loopback address (`127.0.0.1:0` / `[::1]:0`). Without an explicit
/// `bind_addr` hickory binds `0.0.0.0:0` / `[::]:0`.
fn loopback_name_server(addr: SocketAddr) -> Result<NameServerConfig, NetError> {
    if !addr.ip().is_loopback() {
        return Err(NetError(format!(
            "{addr} is not a loopback name server (only mock servers are asked directly)"
        )));
    }
    let mut connection = ConnectionConfig::new(ProtocolConfig::Udp);
    connection.port = addr.port();
    connection.bind_addr = Some(SocketAddr::new(addr.ip(), 0));
    Ok(NameServerConfig::new(addr.ip(), true, vec![connection]))
}

impl DnsLookup for HickoryDns {
    fn srv<'a>(&'a self, name: &'a str) -> BoxFuture<'a, Result<Vec<SrvRecord>, NetError>> {
        Box::pin(async move {
            Ok(self
                .records(name, true)
                .await?
                .into_iter()
                .filter_map(|data| match data {
                    RData::SRV(srv) => Some(SrvRecord {
                        priority: srv.priority,
                        weight: srv.weight,
                        port: srv.port,
                        target: host(&srv.target),
                    }),
                    _ => None,
                })
                .collect())
        })
    }

    fn mx<'a>(&'a self, domain: &'a str) -> BoxFuture<'a, Result<Vec<MxRecord>, NetError>> {
        Box::pin(async move {
            Ok(self
                .records(domain, false)
                .await?
                .into_iter()
                .filter_map(|data| match data {
                    RData::MX(mx) => Some(MxRecord {
                        preference: mx.preference,
                        exchange: host(&mx.exchange),
                    }),
                    _ => None,
                })
                .collect())
        })
    }
}

/// Whether a server answers on a port (for host guessing).
pub trait HostProbe: Send + Sync + fmt::Debug {
    /// `true` when a TCP connection to `host:port` succeeds.
    fn reachable<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, bool>;
}

/// [`HostProbe`] by TCP connect with a timeout. `overrides` maps
/// (host, port) to another address (tests, end-to-end runs against local
/// mock servers); other hosts are not contacted when `only_overrides` is
/// set.
#[derive(Debug, Clone)]
pub struct TcpProbe {
    timeout: Duration,
    overrides: HashMap<(String, u16), SocketAddr>,
    only_overrides: bool,
}

impl TcpProbe {
    /// Probes real hosts.
    pub fn new(timeout: Duration) -> Self {
        Self {
            timeout,
            overrides: HashMap::new(),
            only_overrides: false,
        }
    }

    /// Probes only the overridden hosts, at their override addresses.
    pub fn with_overrides(
        timeout: Duration,
        overrides: HashMap<(String, u16), SocketAddr>,
    ) -> Self {
        Self {
            timeout,
            overrides: overrides
                .into_iter()
                .map(|((h, p), a)| ((h.to_ascii_lowercase(), p), a))
                .collect(),
            only_overrides: true,
        }
    }
}

impl HostProbe for TcpProbe {
    fn reachable<'a>(&'a self, host: &'a str, port: u16) -> BoxFuture<'a, bool> {
        Box::pin(async move {
            let key = (host.to_ascii_lowercase(), port);
            let connect = match self.overrides.get(&key) {
                Some(addr) => tokio::net::TcpStream::connect(*addr),
                None if self.only_overrides => return false,
                None => {
                    // A literal IP is never a guessed host name.
                    if host.parse::<IpAddr>().is_ok() {
                        return false;
                    }
                    let target = format!("{host}:{port}");
                    return tokio::time::timeout(
                        self.timeout,
                        tokio::net::TcpStream::connect(target),
                    )
                    .await
                    .is_ok_and(|r| r.is_ok());
                }
            };
            tokio::time::timeout(self.timeout, connect)
                .await
                .is_ok_and(|r| r.is_ok())
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn routing_rewrites_the_target_and_keeps_the_host() {
        let fetch = ReqwestFetch::new(nail_auth::http_client().unwrap())
            .route_all_to("127.0.0.1:4000".parse().unwrap());
        let url = Url::parse(
            "https://autoconfig.example.org/mail/config-v1.1.xml?emailaddress=a%40example.org",
        )
        .unwrap();
        let (target, host) = fetch.target(&url);
        assert_eq!(
            target.as_str(),
            "http://127.0.0.1:4000/mail/config-v1.1.xml?emailaddress=a%40example.org"
        );
        assert_eq!(host.as_deref(), Some("autoconfig.example.org"));
        let direct = ReqwestFetch::new(nail_auth::http_client().unwrap());
        assert_eq!(direct.target(&url), (url.clone(), None));
    }

    #[test]
    fn mock_name_servers_are_asked_from_loopback_only() {
        for (server, bind) in [("127.0.0.1:5353", "127.0.0.1:0"), ("[::1]:53", "[::1]:0")] {
            let config = loopback_name_server(server.parse().unwrap()).unwrap();
            assert_eq!(config.connections.len(), 1);
            let connection = &config.connections[0];
            assert_eq!(connection.protocol, ProtocolConfig::Udp);
            let bind_addr = connection.bind_addr.unwrap();
            assert_eq!(bind_addr, bind.parse().unwrap());
            assert!(bind_addr.ip().is_loopback() && !bind_addr.ip().is_unspecified());
        }
        for remote in [
            "192.0.2.53:53",
            "0.0.0.0:53",
            "[::]:53",
            "[2001:db8::53]:53",
        ] {
            assert!(
                loopback_name_server(remote.parse().unwrap()).is_err(),
                "{remote}"
            );
            assert!(
                HickoryDns::with_name_server(remote.parse().unwrap(), Duration::from_secs(1))
                    .is_err()
            );
        }
        assert!(
            HickoryDns::with_name_server("127.0.0.1:53".parse().unwrap(), Duration::from_secs(1))
                .is_ok()
        );
    }

    #[tokio::test]
    async fn probes_with_overrides_only() {
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        let probe = TcpProbe::with_overrides(
            Duration::from_secs(2),
            HashMap::from([(("IMAP.example.org".to_owned(), 993), addr)]),
        );
        assert!(probe.reachable("imap.example.org", 993).await);
        assert!(!probe.reachable("imap.example.org", 143).await);
        assert!(!probe.reachable("smtp.example.org", 465).await);
        assert!(
            !TcpProbe::new(Duration::from_secs(1))
                .reachable("127.0.0.1", addr.port())
                .await
        );
    }
}
