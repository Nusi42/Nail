//! A mock CalDAV / CardDAV server (M3a WP72; contract ADR 0058): WebDAV
//! PROPFIND / REPORT (calendar-query, calendar-multiget,
//! addressbook-query / -multiget, sync-collection), conditional PUT /
//! DELETE with ETags, MKCALENDAR, `/.well-known/` redirects, Basic and
//! bearer auth, recorded requests and fault injection. Binds 127.0.0.1
//! only (ADR 0057).
//!
//! Slot created by WP70; WP72 fills it (likely on [`axum`], like
//! [`crate::MockHttpServer`]).

/// The path the mock serves its principal under, by convention.
pub const PRINCIPAL_PATH: &str = "/dav/principals/";

#[cfg(test)]
mod tests {
    #[test]
    fn principal_path_is_absolute() {
        assert!(super::PRINCIPAL_PATH.starts_with('/'));
    }
}
