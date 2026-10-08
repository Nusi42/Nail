//! The well-known ports of the protocols Nail speaks (IANA service names),
//! one definition for the clients, discovery and Autodiscover.

/// `http`.
pub const HTTP: u16 = 80;
/// `https`.
pub const HTTPS: u16 = 443;
/// `submissions`: SMTP submission with implicit TLS (RFC 8314).
pub const SUBMISSIONS: u16 = 465;
/// `submission`: SMTP submission with STARTTLS.
pub const SUBMISSION: u16 = 587;
/// `imaps`: IMAP with implicit TLS.
pub const IMAPS: u16 = 993;
/// `imap`: IMAP with STARTTLS.
pub const IMAP: u16 = 143;
/// `pop3s`: POP3 with implicit TLS.
pub const POP3S: u16 = 995;
/// `pop3`: POP3 with STLS.
pub const POP3: u16 = 110;
// ManageSieve's 4190 is `nail_api::sieve::MANAGESIEVE_DEFAULT_PORT`.

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn implicit_tls_ports_differ_from_their_starttls_ports() {
        assert_ne!(IMAPS, IMAP);
        assert_ne!(POP3S, POP3);
        assert_ne!(SUBMISSIONS, SUBMISSION);
        assert_eq!((HTTPS, IMAPS, POP3S, SUBMISSIONS), (443, 993, 995, 465));
    }
}
