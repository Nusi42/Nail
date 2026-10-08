//! Shared network plumbing of the protocol clients and mock servers (wave
//! Q2, SSOT / ODR / DRY, ADR 0171). Before Q2 the IMAP, POP3, SMTP and
//! ManageSieve clients and the mock servers of `nail-testkit-net` each had
//! their own copy of the same transport code; WP-rust-engines moves one
//! definition here.
//!
//! Planned modules (WP-rust-engines owns this crate):
//!
//! - `transport`: the plain / TLS stream enum with its `AsyncRead` /
//!   `AsyncWrite` forwarding, connect with timeouts, the idle timeout;
//! - `lines`: bounded CRLF line reading (`MAX_LINE`) and literal framing;
//! - `http`: the retry policy (back-off, `Retry-After`) and the loopback /
//!   HTTPS URL policy shared by the Gmail, Graph, EWS and DAV clients.
//!
//! The crate starts empty; dependencies are added with the code (TLS stays
//! rustls + ring, ADR 0014).

#[cfg(test)]
mod tests {
    #[test]
    fn the_crate_builds() {
        assert_eq!(env!("CARGO_PKG_NAME"), "nail-net");
    }
}
