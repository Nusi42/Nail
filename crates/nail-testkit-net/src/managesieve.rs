//! Mock ManageSieve server (RFC 5804) — **slot (WP50) for WP51**
//! (ADR 0043).
//!
//! WP51 builds it like the other mocks here: bound to `127.0.0.1:0`,
//! STARTTLS with [`nail_testkit::tls`], `AUTHENTICATE PLAIN` / `XOAUTH2`,
//! a configurable `SIEVE` extension list, `LISTSCRIPTS`, `CHECKSCRIPT`
//! (with fault injection for a rejected script), `PUTSCRIPT`, `SETACTIVE`,
//! `GETSCRIPT`, `DELETESCRIPT`, recording every stored script and the
//! active one so tests (and the demo-multi seed, WP55) can assert what
//! landed on the server.

#[cfg(test)]
mod tests {
    #[test]
    fn slot_exists() {
        assert_eq!(crate::CRATE, "nail-testkit-net");
    }
}
