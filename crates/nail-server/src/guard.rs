//! Host and Origin allowlist for every route (DNS-rebinding protection).
//!
//! The server listens on loopback without authentication (M0). A page on
//! `http://evil.example:<port>` whose name the attacker rebinds to
//! `127.0.0.1` would otherwise be *same-origin* with the server: its
//! requests carry `Host: evil.example:<port>` and a matching `Origin`. So
//! the server accepts only the loopback names it is actually reachable as
//! (`127.0.0.1`, `localhost`, `[::1]` with the bound port) in `Host`, and
//! the same names in `Origin` when a browser sends one. Anything else gets
//! `403 Forbidden` before a handler runs.

use axum::extract::{Request, State};
use axum::http::{HeaderMap, StatusCode, header};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};

/// The loopback authorities (`host:port`) the server answers to.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct HostAllowlist {
    authorities: [String; 3],
}

impl HostAllowlist {
    /// The allowlist for a server bound to `port`.
    pub(crate) fn new(port: u16) -> Self {
        Self {
            authorities: [
                format!("127.0.0.1:{port}"),
                format!("localhost:{port}"),
                format!("[::1]:{port}"),
            ],
        }
    }

    /// Whether `authority` (a `Host` header value) is one of ours.
    fn allows_authority(&self, authority: &str) -> bool {
        self.authorities
            .iter()
            .any(|a| a.eq_ignore_ascii_case(authority))
    }

    /// Whether `origin` (an `Origin` header value) is `http://` plus one of
    /// our authorities.
    fn allows_origin(&self, origin: &str) -> bool {
        origin
            .strip_prefix("http://")
            .is_some_and(|authority| self.allows_authority(authority))
    }

    /// Whether a request with these headers may be served. `Host` is
    /// required; `Origin` is optional (non-browser clients omit it) but must
    /// be ours when present.
    pub(crate) fn allows(&self, headers: &HeaderMap) -> bool {
        let host_ok = headers
            .get(header::HOST)
            .and_then(|h| h.to_str().ok())
            .is_some_and(|h| self.allows_authority(h));
        let origin_ok = match headers.get(header::ORIGIN) {
            None => true,
            Some(origin) => origin.to_str().is_ok_and(|o| self.allows_origin(o)),
        };
        host_ok && origin_ok
    }
}

/// Middleware: refuses requests whose `Host` or `Origin` is not ours.
pub(crate) async fn check(
    State(allowlist): State<HostAllowlist>,
    request: Request,
    next: Next,
) -> Response {
    if allowlist.allows(request.headers()) {
        next.run(request).await
    } else {
        tracing::warn!(
            host = ?request.headers().get(header::HOST),
            origin = ?request.headers().get(header::ORIGIN),
            uri = %request.uri(),
            "request with a foreign Host or Origin refused"
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
}
