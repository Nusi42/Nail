//! The text shown in place of a secret: in `Debug` output of credentials
//! and in redacted diagnostics, one definition for every crate.

/// The stand-in of a secret value (`Debug` output, diagnostics bundles).
pub const REDACTED: &str = "<redacted>";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_stand_in_reads_as_a_placeholder() {
        assert!(REDACTED.starts_with('<') && REDACTED.ends_with('>'));
        assert!(!REDACTED.contains(char::is_whitespace));
    }
}
