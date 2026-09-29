//! Server capabilities.

use std::collections::BTreeSet;

/// The capability atoms a server advertised (upper-cased).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Capabilities(BTreeSet<String>);

impl Capabilities {
    /// Builds a set from capability atoms, e.g. `["IMAP4rev1", "AUTH=PLAIN"]`.
    pub fn from_atoms<I, S>(atoms: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        Self(
            atoms
                .into_iter()
                .map(|a| a.as_ref().to_ascii_uppercase())
                .collect(),
        )
    }

    /// Whether the capability is present (case-insensitive), e.g. `"SPECIAL-USE"`.
    pub fn has(&self, capability: &str) -> bool {
        self.0.contains(&capability.to_ascii_uppercase())
    }

    /// Whether the SASL mechanism is advertised (`AUTH=<mechanism>`).
    pub fn has_auth(&self, mechanism: &str) -> bool {
        self.has(&format!("AUTH={mechanism}"))
    }

    /// All capabilities, sorted.
    pub fn iter(&self) -> impl Iterator<Item = &str> {
        self.0.iter().map(String::as_str)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn case_insensitive_lookup() {
        let caps = Capabilities::from_atoms(["IMAP4rev1", "auth=plain", "IDLE"]);
        assert!(caps.has("idle"));
        assert!(caps.has_auth("PLAIN"));
        assert!(!caps.has("STARTTLS"));
        assert_eq!(
            caps.iter().collect::<Vec<_>>(),
            ["AUTH=PLAIN", "IDLE", "IMAP4REV1"]
        );
    }
}
