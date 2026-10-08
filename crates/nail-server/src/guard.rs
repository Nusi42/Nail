//! Host and Origin allowlist for every route (DNS-rebinding protection)
//! and a fetch-metadata policy (cross-site request isolation).
//!
//! **Host / Origin.** A page on `http://evil.example:<port>` whose name
//! the attacker rebinds to the server's address would otherwise be
//! *same-origin* with the server: its requests carry `Host:
//! evil.example:<port>` and a matching `Origin`. So the server accepts only
//! the names it is actually reachable as — the loopback names
//! (`127.0.0.1`, `localhost`, `[::1]`) plus, for the remote UI, the
//! configured host name and addresses — with the bound port, in `Host`, and
//! the same names with the server's scheme in `Origin` when a browser sends
//! one. Anything else gets `403 Forbidden` before a handler runs. The
//! remote-content proxy route skips only the `Origin` part: sandboxed mail
//! documents have an opaque origin (`Origin: null` on font requests) and
//! the signed token is the capability there.
//!
//! **Fetch metadata** (`Sec-Fetch-*`, sent by every current browser): a
//! request another site (or the sandbox origin, which is *same-site* but
//! not same-origin) makes to the app is refused unless it is a plain
//! top-level navigation to a page, or a proxied resource. This stops
//! cross-site reads and CSRF-style requests that `SameSite` cookies alone
//! would let through from the sandbox origin, and makes the app unusable
//! as an embedded resource. Requests without the headers (non-browser
//! clients, old browsers) fall back to the Host / Origin checks.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, Method, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use nail_api::remote_content::PROXY_PATH;

/// The authorities (`host:port`) a server answers to, and its scheme.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAllowlist {
    scheme: &'static str,
    authorities: Vec<String>,
}

/// The loopback names every server answers to.
const LOOPBACK_NAMES: [&str; 3] = ["127.0.0.1", "localhost", "::1"];

/// `name` as it appears in an authority: IPv6 addresses in brackets.
fn host_part(name: &str) -> String {
    let name = name.trim_end_matches('.');
    match name.parse::<std::net::Ipv6Addr>() {
        Ok(v6) => format!("[{v6}]"),
        Err(_) => name.to_ascii_lowercase(),
    }
}

impl HostAllowlist {
    /// The loopback allowlist of a plain-HTTP server bound to `port`.
    pub(crate) fn new(port: u16) -> Self {
        Self::with_names("http", &[], port)
    }

    /// The loopback names plus `names` (host names or IP addresses) with
    /// `port`, for `scheme` (`http` / `https`). On the scheme's default
    /// port the bare name is accepted too (browsers omit it).
    pub(crate) fn with_names(scheme: &'static str, names: &[String], port: u16) -> Self {
        let default_port = nail_api::hosts::web_port(scheme == "https");
        let mut authorities = Vec::new();
        for name in LOOPBACK_NAMES
            .iter()
            .map(|n| (*n).to_owned())
            .chain(names.iter().cloned())
        {
            let host = host_part(&name);
            if host.is_empty() {
                continue;
            }
            let with_port = format!("{host}:{port}");
            if !authorities.contains(&with_port) {
                authorities.push(with_port);
            }
            if port == default_port && !authorities.contains(&host) {
                authorities.push(host);
            }
        }
        Self {
            scheme,
            authorities,
        }
    }

    /// The origins (`scheme://authority`) of this server.
    pub(crate) fn origins(&self) -> Vec<String> {
        self.authorities
            .iter()
            .map(|a| format!("{}://{a}", self.scheme))
            .collect()
    }

    /// Whether `authority` (a `Host` header value) is one of ours.
    fn allows_authority(&self, authority: &str) -> bool {
        self.authorities
            .iter()
            .any(|a| a.eq_ignore_ascii_case(authority))
    }

    /// Whether `origin` (an `Origin` header value) is our scheme plus one
    /// of our authorities.
    fn allows_origin(&self, origin: &str) -> bool {
        origin
            .strip_prefix(self.scheme)
            .and_then(|rest| rest.strip_prefix("://"))
            .is_some_and(|authority| self.allows_authority(authority))
    }

    /// Whether the `Host` header names us.
    pub(crate) fn allows_host(&self, headers: &HeaderMap) -> bool {
        headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| self.allows_authority(h))
    }

    /// Whether a request with these headers may be served. `Host` is
    /// required; `Origin` is optional (non-browser clients omit it) but must
    /// be ours when present.
    pub(crate) fn allows(&self, headers: &HeaderMap) -> bool {
        let origin_ok = match headers.get(header::ORIGIN) {
            None => true,
            Some(origin) => origin.to_str().is_ok_and(|o| self.allows_origin(o)),
        };
        self.allows_host(headers) && origin_ok
    }
}

/// How a router checks requests.
#[derive(Debug, Clone)]
pub(crate) struct Guard {
    /// Names, port and scheme.
    pub(crate) hosts: HostAllowlist,
    /// Apply the fetch-metadata policy (the app origin; not the sandbox,
    /// which only serves capability URLs).
    pub(crate) fetch_metadata: bool,
}

/// The fetch-metadata policy (see the module docs). `true` = allowed.
pub(crate) fn fetch_metadata_allows(method: &Method, path: &str, headers: &HeaderMap) -> bool {
    let get = |name: &str| headers.get(name).and_then(|v| v.to_str().ok());
    match get("sec-fetch-site") {
        None | Some("same-origin" | "none") => true,
        Some(_) => {
            if path.starts_with(PROXY_PATH) {
                return true;
            }
            let navigation = get("sec-fetch-mode") == Some("navigate")
                && matches!(get("sec-fetch-dest"), None | Some("document"))
                && (method == Method::GET || method == Method::HEAD);
            navigation && !path.starts_with("/api/")
        }
    }
}

/// Middleware: refuses requests whose `Host` or `Origin` is not ours, or
/// that break the fetch-metadata policy.
pub(crate) async fn check(State(guard): State<Guard>, request: Request, next: Next) -> Response {
    let headers = request.headers();
    let path = request.uri().path();
    let hosts_ok = if path.starts_with(PROXY_PATH) {
        guard.hosts.allows_host(headers)
    } else {
        guard.hosts.allows(headers)
    };
    let metadata_ok =
        !guard.fetch_metadata || fetch_metadata_allows(request.method(), path, headers);
    if hosts_ok && metadata_ok {
        next.run(request).await
    } else {
        tracing::warn!(
            host = ?headers.get(header::HOST),
            origin = ?headers.get(header::ORIGIN),
            site = ?headers.get("sec-fetch-site"),
            uri = %request.uri(),
            "request with a foreign Host, Origin or site refused"
        );
        StatusCode::FORBIDDEN.into_response()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::http::HeaderValue;

    fn headers(host: Option<&str>, origin: Option<&str>) -> HeaderMap {
        let mut h = HeaderMap::new();
        if let Some(v) = host {
            h.insert(header::HOST, HeaderValue::from_str(v).unwrap());
        }
        if let Some(v) = origin {
            h.insert(header::ORIGIN, HeaderValue::from_str(v).unwrap());
        }
        h
    }

    #[test]
    fn loopback_hosts_with_the_bound_port_are_allowed() {
        let list = HostAllowlist::new(4780);
        for host in [
            "127.0.0.1:4780",
            "localhost:4780",
            "LOCALHOST:4780",
            "[::1]:4780",
        ] {
            assert!(list.allows(&headers(Some(host), None)), "{host}");
            let origin = format!("http://{host}");
            assert!(list.allows(&headers(Some(host), Some(&origin))), "{host}");
        }
        // Another loopback name as Origin is fine (same machine, same port).
        assert!(list.allows(&headers(
            Some("127.0.0.1:4780"),
            Some("http://localhost:4780")
        )));
    }

    #[test]
    fn dns_rebinding_is_refused() {
        let list = HostAllowlist::new(4780);
        // The rebinding page: Host and Origin agree, but name the attacker.
        assert!(!list.allows(&headers(
            Some("evil.example:4780"),
            Some("http://evil.example:4780")
        )));
        assert!(!list.allows(&headers(Some("evil.example:4780"), None)));
    }

    #[test]
    fn foreign_or_malformed_values_are_refused() {
        let list = HostAllowlist::new(4780);
        let refused = [
            (None, None),
            (Some("127.0.0.1"), None),
            (Some("127.0.0.1:9999"), None),
            (Some("127.0.0.1:4780"), Some("http://evil.example")),
            (Some("127.0.0.1:4780"), Some("http://127.0.0.1:9999")),
            (Some("127.0.0.1:4780"), Some("https://127.0.0.1:4780")),
            (Some("127.0.0.1:4780"), Some("null")),
            (Some("127.0.0.1:4780.evil.example"), None),
        ];
        for (host, origin) in refused {
            assert!(!list.allows(&headers(host, origin)), "{host:?} {origin:?}");
        }
    }

    #[test]
    fn remote_names_and_https_origins() {
        let names = vec![
            "Nail.Home.Example".to_owned(),
            "192.168.1.5".to_owned(),
            "fe80::1".to_owned(),
        ];
        let list = HostAllowlist::with_names("https", &names, 4780);
        for host in [
            "nail.home.example:4780",
            "192.168.1.5:4780",
            "[fe80::1]:4780",
            "localhost:4780",
        ] {
            let origin = format!("https://{host}");
            assert!(list.allows(&headers(Some(host), Some(&origin))), "{host}");
        }
        // The http scheme is not our origin.
        assert!(!list.allows(&headers(
            Some("nail.home.example:4780"),
            Some("http://nail.home.example:4780")
        )));
        assert!(!list.allows(&headers(Some("nail.home.example"), None)));
        assert!(!list.allows(&headers(Some("evil.example:4780"), None)));
        // Default port: the bare name is accepted too.
        let list = HostAllowlist::with_names("https", &names, 443);
        assert!(list.allows(&headers(
            Some("nail.home.example"),
            Some("https://nail.home.example")
        )));
        assert!(
            list.origins()
                .contains(&"https://nail.home.example".to_owned())
        );
        assert!(list.origins().contains(&"https://[::1]:443".to_owned()));
    }

    #[test]
    fn fetch_metadata_policy() {
        let h = |pairs: &[(&'static str, &'static str)]| {
            let mut map = HeaderMap::new();
            for (n, v) in pairs {
                map.insert(*n, HeaderValue::from_static(v));
            }
            map
        };
        let get = Method::GET;
        // Same origin, direct navigation, or no metadata at all: fine.
        assert!(fetch_metadata_allows(
            &get,
            "/api/blob/x",
            &h(&[("sec-fetch-site", "same-origin")])
        ));
        assert!(fetch_metadata_allows(
            &get,
            "/",
            &h(&[("sec-fetch-site", "none")])
        ));
        assert!(fetch_metadata_allows(&get, "/api/ws", &h(&[])));
        // Another site (or the sandbox origin) using the app: refused.
        for site in ["cross-site", "same-site"] {
            for (path, mode, dest) in [
                ("/api/blob/x", "no-cors", "image"),
                ("/api/ws", "websocket", "websocket"),
                ("/api/auth", "cors", "empty"),
                ("/assets/app.js", "no-cors", "script"),
                ("/", "navigate", "iframe"),
                ("/api/blob/x", "navigate", "document"),
            ] {
                let headers = h(&[
                    ("sec-fetch-site", site),
                    ("sec-fetch-mode", mode),
                    ("sec-fetch-dest", dest),
                ]);
                assert!(
                    !fetch_metadata_allows(&get, path, &headers),
                    "{site} {path} {mode} {dest}"
                );
            }
            // A link to the app from elsewhere still opens it.
            let nav = h(&[
                ("sec-fetch-site", site),
                ("sec-fetch-mode", "navigate"),
                ("sec-fetch-dest", "document"),
            ]);
            assert!(fetch_metadata_allows(&get, "/", &nav));
            assert!(!fetch_metadata_allows(&Method::POST, "/", &nav));
            // Proxied resources are capability URLs loaded by mail documents.
            let img = h(&[
                ("sec-fetch-site", site),
                ("sec-fetch-mode", "no-cors"),
                ("sec-fetch-dest", "image"),
            ]);
            assert!(fetch_metadata_allows(&get, "/api/proxy/abc.def", &img));
        }
    }
}
