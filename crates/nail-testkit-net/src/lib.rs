//! Network mock servers for Nail's tests (M1a).
//!
//! Every server binds to `127.0.0.1:0`, never touches the network, records
//! what it receives and stops when dropped:
//!
//! * [`MockSmtpServer`]: SMTP submission (plaintext / implicit TLS /
//!   STARTTLS, AUTH PLAIN / LOGIN / XOAUTH2 / OAUTHBEARER, SIZE, DSN,
//!   recorded messages, fault injection);
//! * [`MockPop3Server`]: POP3 (STLS, USER/PASS, APOP, SASL, CAPA, UIDL, TOP,
//!   RETR, DELE with commit on QUIT, fault injection);
//! * [`MockOAuthServer`]: OAuth2 authorization code + PKCE and refresh
//!   tokens, on [`axum`];
//! * [`MockHttpServer`]: canned documents by host and path, for autoconfig,
//!   ISPDB and Autodiscover;
//! * [`MockDnsServer`] / [`DnsZone`]: SRV / MX / A answers over UDP
//!   ([`hickory_proto`] messages) or in memory;
//! * [`MockSieveServer`]: ManageSieve (RFC 5804: STARTTLS, AUTHENTICATE
//!   PLAIN / XOAUTH2 / OAUTHBEARER, the script commands, minimal script
//!   validation, recorded uploads, fault injection; M2c WP51);
//! * [`MockDavServer`]: CalDAV / CardDAV (RFC 4791 / 6352: discovery,
//!   PROPFIND, REPORT incl. sync-collection, conditional writes,
//!   MKCALENDAR, Basic / bearer auth, optional TLS, server-side changes
//!   for tests, fault injection; M3a WP72).
//!
//! TLS certificates come from [`nail_testkit::tls`]; clients trust
//! `ca_der()` of the server. The mock IMAP server stays in `nail-testkit`.

#![warn(missing_docs)]

pub mod dav;
pub mod dns;
pub mod http;
pub mod managesieve;
pub mod oauth;
pub mod pop3;
mod sasl;
pub mod smtp;
mod stream;

pub use dav::{
    DavRequest, MockCollection, MockCollectionKind, MockDavConfig, MockDavServer, MockObject,
};
pub use dns::{DnsZone, MockDnsServer, MxAnswer, SrvAnswer, ZoneAnswer};
pub use http::{MockHttpServer, MockResponse, RecordedRequest};
pub use managesieve::{
    MockSieveConfig, MockSieveServer, SieveUpload, StoredScript, validate_script,
};
pub use nail_testkit::MockTls;
pub use oauth::{IssuedToken, MockOAuthConfig, MockOAuthServer, TokenRequest};
pub use pop3::{MockPop3Config, MockPop3Server, Pop3Message, apop_digest};
pub use smtp::{MockSmtpConfig, MockSmtpServer, ReceivedMail, Recipient};

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

/// Errors when starting a mock server.
#[derive(Debug, thiserror::Error)]
pub enum MockNetError {
    /// Binding the socket failed.
    #[error("binding a mock server: {0}")]
    Io(#[from] std::io::Error),
    /// Generating TLS material failed.
    #[error(transparent)]
    Tls(#[from] nail_testkit::tls::TestTlsError),
}

/// Binds a TCP listener on a free loopback port, as every mock server here
/// does.
pub async fn bind_loopback() -> std::io::Result<tokio::net::TcpListener> {
    tokio::net::TcpListener::bind(("127.0.0.1", 0)).await
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn binds_to_loopback_only() {
        let listener = bind_loopback().await.unwrap();
        let addr = listener.local_addr().unwrap();
        assert!(addr.ip().is_loopback());
        assert_ne!(addr.port(), 0);
        assert_eq!(CRATE, "nail-testkit-net");
    }
}
