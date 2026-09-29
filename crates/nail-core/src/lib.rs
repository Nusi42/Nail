//! The Nail service: handles every `nail_api::Request`, owns store, config and sync, and broadcasts `nail_api::Event`s.
//!
//! Stub created by M0 WP0; the implementation lands in its own work package
//! (see `docs/plan.md`).

#[cfg(test)]
mod tests {
    #[test]
    fn crate_compiles() {
        assert_eq!(env!("CARGO_PKG_NAME"), "nail-core");
    }
}
