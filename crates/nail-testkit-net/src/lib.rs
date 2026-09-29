//! Network mock servers for Nail's tests (M1a).
//!
//! **Stub (M1a WP10).** Work package WP12 fills this crate in; all servers
//! bind to `127.0.0.1:0`, never touch the network and record what they
//! receive:
//!
//! * mock **SMTP** submission server (plaintext / implicit TLS / STARTTLS,
//!   AUTH PLAIN / LOGIN / XOAUTH2, SIZE, recorded messages, fault injection);
//! * mock **POP3** server (STLS, USER/PASS, APOP, SASL, UIDL, TOP, RETR, DELE);
//! * mock **OAuth2** authorization server (authorize redirect, token and
//!   refresh endpoints, PKCE check) on [`axum`];
//! * mock **HTTP** server for autoconfig / ISPDB / Autodiscover documents;
//! * mock **DNS** answers for SRV lookups ([`hickory_proto`] messages).
//!
//! TLS certificates come from [`nail_testkit::tls`]. The mock IMAP server
//! stays in `nail-testkit`. Expected shape: `docs/adr/0012-m1a-contract.md`.

#![warn(missing_docs)]

/// Name of the crate, for diagnostics.
pub const CRATE: &str = env!("CARGO_PKG_NAME");

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
    }

    #[test]
    fn dns_messages_encode() {
        use hickory_proto::op::Message;
        let message = Message::query();
        let bytes = message.to_vec().unwrap();
        assert!(bytes.len() >= 12, "DNS header is 12 bytes");
        assert_eq!(CRATE, "nail-testkit-net");
    }
}
