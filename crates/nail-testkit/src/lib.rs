//! Test support: an in-process mock IMAP server, a mail corpus, fixtures and demo seed data.
//!
//! Stub created by M0 WP0; the implementation lands in its own work package
//! (see `docs/plan.md`).

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "nail-testkit");
    }
}
