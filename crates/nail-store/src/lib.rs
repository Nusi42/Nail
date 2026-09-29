//! Local store: SQLite metadata, a content-addressed blob store and the operation queue.
//!
//! Stub created by M0 WP0; the implementation lands in its own work package
//! (see `docs/plan.md`).

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "nail-store");
    }
}
