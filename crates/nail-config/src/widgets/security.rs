//! Widgets of M2a WP34 (security UI, ADRs 0029 and 0033): key and
//! certificate manager, per-identity crypto settings, composer sign /
//! encrypt toggles with recipient key status, the message security and
//! auth-results badges, reader warnings, encryption-at-rest settings and
//! the Autocrypt setup flow. Rendered by `ui/src/features/security/`.

use super::{WidgetSpec, widget};

/// Widgets of this feature (none takes options).
///
/// `security-keys`: key and certificate manager; `security-identities`:
/// per-identity crypto settings; `security-settings`: encryption at rest,
/// key lookup, decrypted indexing; `security-autocrypt`: setup message
/// send / import (the import dialog is a UI dialog, not a widget);
/// `security-message`: badges of the selected message;
/// `security-warnings`: link and attachment warnings of the selected
/// message; `security-composer`: sign / encrypt toggles with recipient
/// key status.
pub(crate) const WIDGETS: &[WidgetSpec] = &[
    widget("security-keys", &[], &[], false),
    widget("security-identities", &[], &[], false),
    widget("security-settings", &[], &[], false),
    widget("security-autocrypt", &[], &[], false),
    widget("security-message", &[], &[], false),
    widget("security-warnings", &[], &[], false),
    widget("security-composer", &[], &[], false),
    // M4c (WP124, ADR 0094): future widgets (any options) — the vault
    // recovery key (create / show once / verify / restore) and smartcards
    // (PKCS#11 modules, tokens, keys, PIN cache).
    widget("security-recovery-key", &[], &[], false),
    widget("security-smartcards", &[], &[], false),
];

#[cfg(test)]
mod tests {
    use crate::widgets::testing::parses;

    #[test]
    fn security_widgets_parse() {
        let kinds: Vec<&str> = super::WIDGETS.iter().map(|w| w.kind).collect();
        assert!(parses(&kinds));
        assert!(!parses(&["security-keys bogus=#true"]));
    }
}
